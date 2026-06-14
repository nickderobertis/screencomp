//! `screencomp scope` — match a changed-path list against `[guard].paths`.
//!
//! This is the small, pure decision the local pre-push guard needs: given the
//! set of files a push would change, are any of them screenshot-relevant? It
//! reads the globs from config and the candidate paths from stdin (or a file),
//! then reports whether any matched. It touches no git, network, or working-tree
//! state — only string matching — so the slow capture path runs only when it
//! genuinely should.

use std::io::{Read as _, Write};

use serde::Serialize;

use super::{Ctx, load_config, write_err};
use crate::cli::{OutputFormat, ScopeArgs};
use crate::domain::scope::any_match;
use crate::errors::AppError;
use crate::io::fs;

pub(crate) fn run(args: &ScopeArgs, ctx: &Ctx, out: &mut dyn Write) -> Result<i32, AppError> {
    // Config (including any auto-discovered screencomp.toml) is resolved at the boundary.
    let cfg = load_config(args.config.as_deref())?;

    let input = read_candidates(args)?;
    // Trim and drop blanks so a trailing newline or stray empty line is harmless.
    let candidates: Vec<&str> = input
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();

    let matched: Vec<&str> = candidates
        .iter()
        .copied()
        .filter(|path| any_match(&cfg.guard.paths, path))
        .collect();
    let any = !matched.is_empty();

    match args.format {
        OutputFormat::Json => write_json(&matched, candidates.len(), out)?,
        OutputFormat::Human if !ctx.quiet => write_human(&matched, candidates.len(), out)?,
        OutputFormat::Human => {}
    }

    if args.exit_code && any {
        return Ok(3);
    }
    Ok(0)
}

/// Read the candidate paths from stdin (`-`) or a file. Stdin is process I/O
/// read at the boundary; a file read goes through [`crate::io`].
fn read_candidates(args: &ScopeArgs) -> Result<String, AppError> {
    if args.changed_from.as_str() == "-" {
        let mut buf = String::new();
        std::io::stdin()
            .read_to_string(&mut buf)
            .map_err(|e| AppError::io("reading paths from stdin", e))?;
        Ok(buf)
    } else {
        fs::read_text(&args.changed_from)
    }
}

/// Stable single-line JSON contract for the hook (and any automation).
fn write_json(matched: &[&str], considered: usize, out: &mut dyn Write) -> Result<(), AppError> {
    #[derive(Serialize)]
    struct Report<'a> {
        matched: bool,
        considered: usize,
        paths: &'a [&'a str],
    }

    let report = Report {
        matched: !matched.is_empty(),
        considered,
        paths: matched,
    };
    let json = serde_json::to_string(&report)
        .map_err(|e| AppError::io("serializing JSON", std::io::Error::other(e)))?;
    writeln!(out, "{json}").map_err(write_err)
}

/// One line per matched path, then a single summary line.
fn write_human(matched: &[&str], considered: usize, out: &mut dyn Write) -> Result<(), AppError> {
    for path in matched {
        writeln!(out, "match {path}").map_err(write_err)?;
    }
    writeln!(
        out,
        "{} of {considered} changed paths are screenshot-relevant",
        matched.len()
    )
    .map_err(write_err)
}
