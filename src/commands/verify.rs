//! `screencomp verify` — the reproducibility gate.
//!
//! Capturing the *same* build twice and requiring byte-identical output is what
//! makes image-free baselines safe: a committed digest is only meaningful if the
//! capture pipeline is deterministic on the machine that produced it. This
//! command is that check as a first-class operation — it compares two captures
//! and exits non-zero (`3`) the moment any shot diverges, so a flaky pipeline
//! fails loudly instead of silently poisoning the baseline.
//!
//! It reuses `classify`'s digest comparison but reframes the result: between two
//! runs of one build there is no "added"/"removed" change, only *divergence*, so
//! the vocabulary is divergence-kind, not diff-status.

use std::collections::BTreeMap;
use std::io::Write;

use serde::Serialize;

use super::{Ctx, discover_scoped, resolve_arch, write_err};
use crate::cli::{OutputFormat, VerifyArgs};
use crate::domain::classify::{Classification, Status, classify};
use crate::errors::AppError;

pub(crate) fn run(args: &VerifyArgs, ctx: &Ctx, out: &mut dyn Write) -> Result<i32, AppError> {
    let arch = resolve_arch(args.arch.as_deref(), &ctx.config.capture.arches)?;
    let plat = arch.as_deref();
    let first = discover_scoped(&args.first, plat)?;
    let second = discover_scoped(&args.second, plat)?;
    // `first` plays the baseline role: a shot only in `first` is `Removed`
    // (only-in-first), one only in `second` is `Added` (only-in-second).
    let result = classify(&first, &second, &[]);

    match args.format {
        OutputFormat::Json => write_json(&result, out)?,
        OutputFormat::Human if !ctx.quiet => write_human(&result, out)?,
        OutputFormat::Human => {}
    }

    if result.has_changes() { Ok(3) } else { Ok(0) }
}

/// How a single shot diverged between the two runs.
fn divergence_kind(status: Status) -> &'static str {
    match status {
        Status::Changed => "differs",
        Status::Removed => "only-in-first",
        Status::Added => "only-in-second",
        Status::Unchanged => "identical",
    }
}

/// Stable single-line JSON contract for automation.
fn write_json(result: &Classification, out: &mut dyn Write) -> Result<(), AppError> {
    #[derive(Serialize)]
    struct Divergent<'a> {
        name: &'a str,
        toggles: &'a BTreeMap<String, String>,
        kind: &'a str,
    }
    #[derive(Serialize)]
    struct Report<'a> {
        reproducible: bool,
        checked: usize,
        divergent: Vec<Divergent<'a>>,
    }

    let divergent = result
        .entries
        .iter()
        .filter(|e| e.status != Status::Unchanged)
        .map(|e| Divergent {
            name: &e.key.name,
            toggles: &e.key.toggles,
            kind: divergence_kind(e.status),
        })
        .collect();
    let report = Report {
        reproducible: !result.has_changes(),
        checked: result.entries.len(),
        divergent,
    };
    let json = serde_json::to_string(&report)
        .map_err(|e| AppError::io("serializing JSON", std::io::Error::other(e)))?;
    writeln!(out, "{json}").map_err(write_err)
}

/// One line per divergent shot, then a single pass/fail summary line.
fn write_human(result: &Classification, out: &mut dyn Write) -> Result<(), AppError> {
    for entry in &result.entries {
        if entry.status != Status::Unchanged {
            writeln!(
                out,
                "{} {}",
                divergence_kind(entry.status),
                entry.key.label()
            )
            .map_err(write_err)?;
        }
    }
    let c = result.counts;
    if result.has_changes() {
        writeln!(
            out,
            "NOT reproducible: {} differ, {} only in first run, {} only in second (of {})",
            c.changed,
            c.removed,
            c.added,
            result.entries.len()
        )
        .map_err(write_err)
    } else {
        writeln!(out, "reproducible: {} shots byte-identical", c.unchanged).map_err(write_err)
    }
}
