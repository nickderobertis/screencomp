//! `screencomp classify` — compare a current capture against a baseline.

use std::io::Write;

use std::collections::BTreeMap;

use serde::Serialize;

use super::{Ctx, baseline_snapshot, discover_scoped, resolve_arch, write_err};
use crate::cli::{ClassifyArgs, OutputFormat};
use crate::domain::classify::{Classification, Counts, Status, classify};
use crate::errors::AppError;

pub(crate) fn run(args: &ClassifyArgs, ctx: &Ctx, out: &mut dyn Write) -> Result<i32, AppError> {
    let arch = resolve_arch(args.arch.as_deref(), &ctx.config.capture.arches)?;
    let arch = arch.as_deref();
    let baseline = baseline_snapshot(
        args.baseline.as_deref(),
        args.baseline_manifest.as_deref(),
        arch,
    )?;
    let current = discover_scoped(&args.current, arch)?;
    let classification = classify(&baseline, &current);

    match args.format {
        OutputFormat::Json => write_json(&classification, out)?,
        OutputFormat::Human if !ctx.quiet => write_human(&classification, out)?,
        OutputFormat::Human => {}
    }

    if args.exit_code && classification.has_changes() {
        return Ok(3);
    }
    Ok(0)
}

/// One shot in the JSON contract: name, toggle map, and status.
#[derive(Serialize)]
struct EntryView<'a> {
    name: &'a str,
    toggles: &'a BTreeMap<String, String>,
    status: &'a str,
}

/// Stable single-line JSON contract for automation.
fn write_json(classification: &Classification, out: &mut dyn Write) -> Result<(), AppError> {
    #[derive(Serialize)]
    struct Report<'a> {
        entries: Vec<EntryView<'a>>,
        counts: Counts,
        changed: bool,
    }

    let report = Report {
        entries: classification
            .entries
            .iter()
            .map(|e| EntryView {
                name: &e.key.name,
                toggles: &e.key.toggles,
                status: e.status.label_lower(),
            })
            .collect(),
        counts: classification.counts,
        changed: classification.has_changes(),
    };
    let json = serde_json::to_string(&report)
        .map_err(|e| AppError::io("serializing JSON", std::io::Error::other(e)))?;
    writeln!(out, "{json}").map_err(write_err)
}

/// One line per changed shot, then a single summary line.
fn write_human(classification: &Classification, out: &mut dyn Write) -> Result<(), AppError> {
    for entry in &classification.entries {
        if entry.status != Status::Unchanged {
            writeln!(out, "{} {}", entry.status.label_lower(), entry.key.label())
                .map_err(write_err)?;
        }
    }
    let c = classification.counts;
    writeln!(
        out,
        "added {} changed {} removed {} unchanged {}",
        c.added, c.changed, c.removed, c.unchanged
    )
    .map_err(write_err)
}
