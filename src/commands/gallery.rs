//! `screencomp gallery` — build a static HTML index of a screenshot tree.

use std::io::Write;

use super::{Ctx, write_err};
use crate::cli::GalleryArgs;
use crate::domain::gallery::render_html;
use crate::errors::AppError;
use crate::io::fs;

pub(crate) fn run(args: &GalleryArgs, ctx: &Ctx, out: &mut dyn Write) -> Result<i32, AppError> {
    let snapshot = fs::discover(&args.input)?;
    let html = render_html(&snapshot, &args.title);

    let index = args.output.join("index.html");
    fs::write_string(&index, &html)?;

    if !ctx.quiet {
        writeln!(out, "wrote {index}").map_err(write_err)?;
    }
    Ok(0)
}
