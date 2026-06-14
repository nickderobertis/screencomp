//! `screencomp doctor` — preflight a capture before classifying.
//!
//! The two surprises a first-time integrator hits are an unexpected platform key
//! (so `--platform auto` scopes to an empty subtree) and a capture written to the
//! wrong path (so the tree is silently ignored) — both of which surface only as a
//! confusing *empty diff* downstream. This command makes them explicit up front:
//! it prints the resolved platform key and the exact path it inspected, lists the
//! projects and shot counts it found, and flags layout problems (no shots, or
//! `.png` files stranded at the root). With `--exit-code` it doubles as a CI gate.

use std::io::Write;

use camino::Utf8Path;
use serde::Serialize;

use super::{Ctx, discover_scoped, platform, scan_scoped, write_err};
use crate::cli::{DoctorArgs, OutputFormat};
use crate::domain::classify::classify;
use crate::domain::layout::LayoutScan;
use crate::errors::AppError;
use crate::io::fs;

pub(crate) fn run(args: &DoctorArgs, ctx: &Ctx, out: &mut dyn Write) -> Result<i32, AppError> {
    let plat = args.platform.as_deref();
    // Resolve the platform key once so the report shows what `auto` became and
    // which subtree was actually scanned.
    let resolved = plat.map(platform::resolve);
    let scoped = platform::scope(&args.input, plat);
    // A missing scoped directory is the same hard error every command raises, so
    // a typo in `--platform` fails identically here (with a layout hint).
    let scan = scan_scoped(&args.input, plat)?;

    // Optional sanity check against a committed manifest: catch the "everything
    // changed" platform mismatch before it reaches classify.
    let warnings = match args.baseline_manifest.as_deref() {
        Some(manifest) => baseline_warnings(manifest, &args.input, plat, resolved.as_deref())?,
        None => Vec::new(),
    };

    match args.format {
        OutputFormat::Json => {
            write_json(
                &args.input,
                resolved.as_deref(),
                &scoped,
                &scan,
                &warnings,
                out,
            )?;
        }
        OutputFormat::Human if !ctx.quiet => {
            write_human(plat, resolved.as_deref(), &scoped, &scan, &warnings, out)?;
        }
        OutputFormat::Human => {}
    }

    if args.exit_code && scan.has_problems() {
        return Ok(3);
    }
    Ok(0)
}

/// Compare the scoped capture against a committed digest `manifest` and return
/// advisory warnings for the two ways a baseline silently lies.
///
/// A screenshot's bytes depend on the OS, CPU, and fonts that rendered it, so a
/// baseline captured elsewhere makes *every* shot look changed — the single most
/// confusing first-run failure. This flags it two ways: when the manifest's
/// `<platform>.sha256` filename names a platform other than the capture's, and
/// when the comparison finds shared shots but zero unchanged. Both are advisory
/// (they never fail the gate), since a legitimately total rewrite looks the same.
fn baseline_warnings(
    manifest: &camino::Utf8Path,
    input: &camino::Utf8Path,
    plat: Option<&str>,
    resolved: Option<&str>,
) -> Result<Vec<String>, AppError> {
    let baseline = fs::read_manifest(manifest)?;
    let current = discover_scoped(input, plat)?;
    let host = platform::host_key();
    // The platform the capture is meant to represent: the resolved key when
    // scoped, else the host running this binary.
    let reference = resolved.unwrap_or(&host);

    let mut warnings = Vec::new();

    // The manifest filename encodes its platform by convention (linux-x86_64.sha256).
    if let Some(stem) = manifest.file_stem()
        && looks_like_platform_key(stem)
        && stem != reference
    {
        warnings.push(format!(
            "baseline manifest '{stem}' names a different platform than the capture ({reference}); \
             digests differ per OS/arch/fonts, so compare like-for-like"
        ));
    }

    // Shared shot names but nothing identical: the byte-for-byte mismatch a
    // wrong-platform baseline produces.
    let c = classify(&baseline, &current).counts;
    if c.unchanged == 0 && c.changed > 0 {
        warnings.push(format!(
            "every shared shot differs from the baseline ({} changed, 0 unchanged) — usually a \
             platform/environment mismatch, not a real change; confirm the baseline was captured \
             on {host}",
            c.changed
        ));
    }

    Ok(warnings)
}

/// Whether `stem` looks like an `<os>-<arch>` platform key, so a manifest named
/// after something else (e.g. `baseline.sha256`) is not mistaken for one.
fn looks_like_platform_key(stem: &str) -> bool {
    matches!(
        stem.split_once('-'),
        Some((os, arch))
            if !arch.is_empty()
                && matches!(os, "linux" | "macos" | "windows" | "freebsd")
    )
}

/// Stable single-line JSON contract for automation.
fn write_json(
    input: &Utf8Path,
    resolved: Option<&str>,
    scoped: &Utf8Path,
    scan: &LayoutScan,
    warnings: &[String],
    out: &mut dyn Write,
) -> Result<(), AppError> {
    #[derive(Serialize)]
    struct Project<'a> {
        name: &'a str,
        shots: usize,
    }
    #[derive(Serialize)]
    struct Report<'a> {
        input: &'a str,
        platform: Option<&'a str>,
        inspected: &'a str,
        projects: Vec<Project<'a>>,
        loose_pngs: &'a [String],
        total_shots: usize,
        warnings: &'a [String],
        ok: bool,
    }

    let report = Report {
        input: input.as_str(),
        platform: resolved,
        inspected: scoped.as_str(),
        projects: scan
            .projects
            .iter()
            .map(|p| Project {
                name: &p.name,
                shots: p.shots,
            })
            .collect(),
        loose_pngs: &scan.loose_pngs,
        total_shots: scan.total_shots(),
        warnings,
        ok: !scan.has_problems(),
    };
    let json = serde_json::to_string(&report)
        .map_err(|e| AppError::io("serializing JSON", std::io::Error::other(e)))?;
    writeln!(out, "{json}").map_err(write_err)
}

/// Human-readable preflight report: resolved platform, scanned path, projects,
/// and any layout problems, ending in a single verdict line.
fn write_human(
    plat: Option<&str>,
    resolved: Option<&str>,
    scoped: &Utf8Path,
    scan: &LayoutScan,
    warnings: &[String],
    out: &mut dyn Write,
) -> Result<(), AppError> {
    match resolved {
        // `auto` resolved to a concrete key worth showing explicitly.
        Some(key) if plat == Some(platform::AUTO) => writeln!(out, "platform: {key} (auto)"),
        Some(key) => writeln!(out, "platform: {key}"),
        // No platform layer: the root is treated as project-level.
        None => writeln!(out, "platform: none (root is project-level)"),
    }
    .map_err(write_err)?;
    writeln!(out, "inspected: {scoped}").map_err(write_err)?;

    writeln!(out, "projects: {}", scan.projects.len()).map_err(write_err)?;
    for project in &scan.projects {
        writeln!(
            out,
            "  {} ({} {})",
            project.name,
            project.shots,
            if project.shots == 1 { "shot" } else { "shots" }
        )
        .map_err(write_err)?;
    }
    writeln!(out, "shots: {}", scan.total_shots()).map_err(write_err)?;

    if !scan.loose_pngs.is_empty() {
        writeln!(
            out,
            "warning: {} .png file(s) directly under the root (expected <project>/<name>.png): {}",
            scan.loose_pngs.len(),
            scan.loose_pngs.join(", ")
        )
        .map_err(write_err)?;
    }
    if scan.total_shots() == 0 {
        writeln!(out, "warning: no screenshots found under {scoped}").map_err(write_err)?;
    }
    for warning in warnings {
        writeln!(out, "warning: {warning}").map_err(write_err)?;
    }

    if scan.has_problems() {
        writeln!(out, "problems found: layout will not classify as expected")
    } else {
        writeln!(out, "ok: layout matches <project>/<name>.png")
    }
    .map_err(write_err)
}
