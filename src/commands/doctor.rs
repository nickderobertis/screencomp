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

use super::{Ctx, platform, scan_scoped, write_err};
use crate::cli::{DoctorArgs, OutputFormat};
use crate::domain::layout::LayoutScan;
use crate::errors::AppError;

pub(crate) fn run(args: &DoctorArgs, ctx: &Ctx, out: &mut dyn Write) -> Result<i32, AppError> {
    let plat = args.platform.as_deref();
    // Resolve the platform key once so the report shows what `auto` became and
    // which subtree was actually scanned.
    let resolved = plat.map(platform::resolve);
    let scoped = platform::scope(&args.input, plat);
    // A missing scoped directory is the same hard error every command raises, so
    // a typo in `--platform` fails identically here (with a layout hint).
    let scan = scan_scoped(&args.input, plat)?;

    match args.format {
        OutputFormat::Json => write_json(&args.input, resolved.as_deref(), &scoped, &scan, out)?,
        OutputFormat::Human if !ctx.quiet => {
            write_human(plat, resolved.as_deref(), &scoped, &scan, out)?;
        }
        OutputFormat::Human => {}
    }

    if args.exit_code && scan.has_problems() {
        return Ok(3);
    }
    Ok(0)
}

/// Stable single-line JSON contract for automation.
fn write_json(
    input: &Utf8Path,
    resolved: Option<&str>,
    scoped: &Utf8Path,
    scan: &LayoutScan,
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

    if scan.has_problems() {
        writeln!(out, "problems found: layout will not classify as expected")
    } else {
        writeln!(out, "ok: layout matches <project>/<name>.png")
    }
    .map_err(write_err)
}
