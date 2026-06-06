//! `screencomp gallery` — build a static HTML gallery of a screenshot tree, or a
//! before/after diff gallery when a baseline is supplied.

use std::io::Write;

use super::{Ctx, write_err};
use crate::cli::GalleryArgs;
use crate::domain::classify::classify;
use crate::domain::gallery::{render_diff_html, render_html};
use crate::errors::AppError;
use crate::io::fs;

pub(crate) fn run(args: &GalleryArgs, ctx: &Ctx, out: &mut dyn Write) -> Result<i32, AppError> {
    let current = fs::discover(&args.input)?;
    let index = args.output.join("index.html");

    match &args.baseline {
        // Plain gallery of a single tree.
        None => {
            let html = render_html(&current, &args.title);
            fs::write_string(&index, &html)?;
            // Copy the referenced images next to index.html so the output is a
            // self-contained, deploy-ready directory.
            let images = fs::copy_png_tree(&args.input, &args.output)?;
            if !ctx.quiet {
                writeln!(out, "wrote {index} ({images} images)").map_err(write_err)?;
            }
        }
        // Before/after diff gallery of current against baseline.
        Some(baseline_dir) => {
            let baseline = fs::discover(baseline_dir)?;
            let classification = classify(&baseline, &current);
            let html = render_diff_html(&classification, &args.title);
            fs::write_string(&index, &html)?;
            // Both trees are referenced by the diff page (before and after).
            let base = fs::copy_png_tree(baseline_dir, &args.output.join("baseline"))?;
            let cur = fs::copy_png_tree(&args.input, &args.output.join("current"))?;
            if !ctx.quiet {
                writeln!(
                    out,
                    "wrote {index} (diff: {base} baseline, {cur} current images)"
                )
                .map_err(write_err)?;
            }
        }
    }

    Ok(0)
}
