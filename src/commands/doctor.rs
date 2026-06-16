//! `screencomp doctor` — preflight a capture before classifying.
//!
//! The two surprises a first-time integrator hits are an unexpected arch (so
//! `--arch auto` scopes to an empty subtree) and a capture written to the wrong
//! path (so the tree is silently ignored) — both of which surface only as a
//! confusing *empty diff* downstream. This command makes them explicit up front:
//! it prints the resolved arch and the exact path it inspected, lists the
//! projects and shot counts it found, and flags layout problems (no shots, or
//! `.png` files stranded at the root). With `--exit-code` it doubles as a CI gate.

use std::io::Write;

use camino::Utf8Path;
use serde::Serialize;

use super::{Ctx, arch, discover_scoped, resolve_arch, scan_scoped, write_err};
use crate::cli::{DoctorArgs, OutputFormat};
use crate::domain::classify::classify;
use crate::domain::layout::LayoutScan;
use crate::errors::AppError;
use crate::io::fs;

pub(crate) fn run(args: &DoctorArgs, ctx: &Ctx, out: &mut dyn Write) -> Result<i32, AppError> {
    // Resolve the arch once (an explicit `--arch`, else the host default when
    // `[capture].arches` is configured) so the report shows what was scoped and
    // which subtree was actually scanned.
    let resolved = resolve_arch(args.arch.as_deref(), &ctx.config.capture.arches)?;
    let plat = resolved.as_deref();
    // Whether the arch was auto-detected from the host (explicit `auto`, or the
    // config-default when no `--arch` was passed) versus named explicitly.
    let auto =
        resolved.is_some() && (args.arch.is_none() || args.arch.as_deref() == Some(arch::AUTO));
    let scoped = arch::scope(&args.input, plat);
    // A missing scoped directory is the same hard error every command raises, so
    // a wrong `--arch` fails identically here (with a layout hint).
    let scan = scan_scoped(&args.input, plat)?;

    // Optional sanity check against a committed manifest: catch the "everything
    // changed" arch mismatch before it reaches classify.
    let warnings = match args.baseline_manifest.as_deref() {
        Some(manifest) => baseline_warnings(manifest, &args.input, plat)?,
        None => Vec::new(),
    };

    match args.format {
        OutputFormat::Json => {
            write_json(&args.input, plat, &scoped, &scan, &warnings, out)?;
        }
        OutputFormat::Human if !ctx.quiet => {
            write_human(plat, auto, &scoped, &scan, &warnings, out)?;
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
/// A screenshot's bytes depend on the CPU arch and fonts that rendered it, so a
/// baseline captured on another arch makes *every* shot look changed — the
/// single most confusing first-run failure. This flags it two ways: when the
/// manifest's `<arch>.sha256` filename names an arch other than the capture's,
/// and when the comparison finds shared shots but zero unchanged. Both are
/// advisory (they never fail the gate), since a legitimately total rewrite looks
/// the same.
fn baseline_warnings(
    manifest: &Utf8Path,
    input: &Utf8Path,
    plat: Option<&str>,
) -> Result<Vec<String>, AppError> {
    let baseline = fs::read_manifest(manifest)?;
    let current = discover_scoped(input, plat)?;
    let host = arch::host_arch();
    // The arch the capture is meant to represent: the resolved arch when scoped,
    // else the host running this binary.
    let reference = plat.unwrap_or(&host);

    let mut warnings = Vec::new();

    // The manifest filename encodes its arch by convention (x86_64.sha256).
    if let Some(stem) = manifest.file_stem()
        && looks_like_arch_key(stem)
        && arch::canonical(stem) != reference
    {
        warnings.push(format!(
            "baseline manifest '{stem}' names a different arch than the capture ({reference}); \
             digests differ per arch/fonts, so compare like-for-like"
        ));
    }

    // Shared shot names but nothing identical: the byte-for-byte mismatch a
    // wrong-arch baseline produces.
    let c = classify(&baseline, &current).counts;
    if c.unchanged == 0 && c.changed > 0 {
        warnings.push(format!(
            "every shared shot differs from the baseline ({} changed, 0 unchanged) — usually an \
             arch/environment mismatch, not a real change; confirm the baseline was captured \
             on {host}",
            c.changed
        ));
    }

    Ok(warnings)
}

/// Whether `stem` looks like a CPU-arch key, so a manifest named after something
/// else (e.g. `baseline.sha256`) is not mistaken for one.
fn looks_like_arch_key(stem: &str) -> bool {
    matches!(stem, "x86_64" | "arm64" | "aarch64")
}

/// Stable single-line JSON contract for automation.
fn write_json(
    input: &Utf8Path,
    arch: Option<&str>,
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
        arch: Option<&'a str>,
        inspected: &'a str,
        projects: Vec<Project<'a>>,
        loose_pngs: &'a [String],
        total_shots: usize,
        warnings: &'a [String],
        ok: bool,
    }

    let report = Report {
        input: input.as_str(),
        arch,
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

/// Human-readable preflight report: resolved arch, scanned path, projects, and
/// any layout problems, ending in a single verdict line.
fn write_human(
    arch: Option<&str>,
    auto: bool,
    scoped: &Utf8Path,
    scan: &LayoutScan,
    warnings: &[String],
    out: &mut dyn Write,
) -> Result<(), AppError> {
    match arch {
        // Arch auto-detected from the host (explicit `auto` or the config default).
        Some(key) if auto => writeln!(out, "arch: {key} (auto)"),
        Some(key) => writeln!(out, "arch: {key}"),
        // No arch layer: the root is treated as project-level.
        None => writeln!(out, "arch: none (root is project-level)"),
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
