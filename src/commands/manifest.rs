//! `screencomp manifest` — write a digest manifest for a screenshot tree.

use std::io::Write;

use super::{Ctx, discover_scoped, write_err};
use crate::cli::ManifestArgs;
use crate::domain::manifest::render_manifest;
use crate::errors::AppError;
use crate::io::fs;

pub(crate) fn run(args: &ManifestArgs, ctx: &Ctx, out: &mut dyn Write) -> Result<i32, AppError> {
    let snapshot = discover_scoped(&args.input, args.platform.as_deref())?;
    let manifest = render_manifest(&snapshot);

    match &args.output {
        Some(path) => {
            fs::write_string(path, &manifest)?;
            if !ctx.quiet {
                writeln!(out, "wrote {path}").map_err(write_err)?;
            }
        }
        None => write!(out, "{manifest}").map_err(write_err)?,
    }
    Ok(0)
}
