//! `screencomp classify` — compare a current capture against a baseline.

use std::io::Write;

use serde::Serialize;

use super::{Ctx, platform, write_err};
use crate::cli::{ClassifyArgs, OutputFormat};
use crate::domain::classify::{Classification, Counts, Entry, Status, classify};
use crate::errors::AppError;
use crate::io::fs;

pub(crate) fn run(args: &ClassifyArgs, ctx: &Ctx, out: &mut dyn Write) -> Result<i32, AppError> {
    let plat = args.platform.as_deref();
    let baseline = fs::discover(&platform::scope(&args.baseline, plat))?;
    let current = fs::discover(&platform::scope(&args.current, plat))?;
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

/// Stable single-line JSON contract for automation.
fn write_json(classification: &Classification, out: &mut dyn Write) -> Result<(), AppError> {
    #[derive(Serialize)]
    struct Report<'a> {
        entries: &'a [Entry],
        counts: Counts,
        changed: bool,
    }

    let report = Report {
        entries: &classification.entries,
        counts: classification.counts,
        changed: classification.has_changes(),
    };
    let json = serde_json::to_string(&report)
        .map_err(|e| AppError::io("serializing JSON", std::io::Error::other(e)))?;
    writeln!(out, "{json}").map_err(write_err)
}

/// One line per changed screenshot, then a single summary line.
fn write_human(classification: &Classification, out: &mut dyn Write) -> Result<(), AppError> {
    for entry in &classification.entries {
        if entry.status != Status::Unchanged {
            writeln!(
                out,
                "{} {}/{}",
                entry.status.label_lower(),
                entry.project,
                entry.name
            )
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
