//! `screencomp arches` — print the project's configured `[capture].arches`.
//!
//! The single source of truth for which CPU architectures a project maintains
//! screenshots for lives in `screencomp.toml`. CI reads it here (`--format json`)
//! to fan out one capture lane per arch, so the matrix and the local default both
//! derive from one committed list. The output is the deliverable, so it is
//! written regardless of `--quiet`.

use std::io::Write;

use super::{Ctx, write_err};
use crate::cli::{ArchesArgs, OutputFormat};
use crate::errors::AppError;

pub(crate) fn run(args: &ArchesArgs, ctx: &Ctx, out: &mut dyn Write) -> Result<i32, AppError> {
    let arches = &ctx.config.capture.arches;
    match args.format {
        OutputFormat::Json => {
            let json = serde_json::to_string(arches)
                .map_err(|e| AppError::io("serializing JSON", std::io::Error::other(e)))?;
            writeln!(out, "{json}").map_err(write_err)?;
        }
        OutputFormat::Human => {
            for arch in arches {
                writeln!(out, "{arch}").map_err(write_err)?;
            }
        }
    }
    Ok(0)
}
