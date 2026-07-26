//! `screencomp gallery` — build a static HTML gallery of a screenshot tree, or a
//! before/after diff gallery when a baseline is supplied.

use std::io::Write;

use super::{Ctx, arch, discover_scoped, resolve_arch, write_err};
use crate::cli::GalleryArgs;
use crate::domain::classify::classify;
use crate::domain::gallery::{DiffMode, render_diff_html, render_html};
use crate::errors::AppError;
use crate::io::fs;

pub(crate) fn run(args: &GalleryArgs, ctx: &Ctx, out: &mut dyn Write) -> Result<i32, AppError> {
    let resolved = resolve_arch(args.arch.as_deref(), &ctx.config.capture.arches)?;
    let plat = resolved.as_deref();
    let input_root = arch::scope(&args.input, plat);
    let current = discover_scoped(&args.input, plat)?;
    let index = args.output.join("index.html");

    match &args.baseline {
        // Plain gallery of a single capture, with toggle controls.
        None => {
            let html = render_html(&current, &ctx.config.toggles, &args.title);
            fs::write_string(&index, &html)?;
            fs::copy_index(&input_root, &args.output)?;
            // Copy the referenced images next to index.html so the output is a
            // self-contained, deploy-ready directory.
            let images = fs::copy_images(&input_root, &args.output, &current)?;
            if !ctx.quiet {
                writeln!(out, "wrote {index} ({images} images)").map_err(write_err)?;
            }
        }
        // Before/after diff gallery of current against baseline.
        Some(baseline_dir) => {
            let baseline_root = arch::scope(baseline_dir, plat);
            let baseline = discover_scoped(baseline_dir, plat)?;
            let classification = classify(&baseline, &current, &[]);
            let mode = if args.focused {
                DiffMode::Focused
            } else {
                DiffMode::Full
            };
            let html = render_diff_html(&classification, &args.title, mode);
            fs::write_string(&index, &html)?;
            fs::copy_index(&baseline_root, &args.output.join("baseline"))?;
            fs::copy_index(&input_root, &args.output.join("current"))?;
            // Both captures are referenced by the diff page (before and after).
            let base = fs::copy_images(&baseline_root, &args.output.join("baseline"), &baseline)?;
            let cur = fs::copy_images(&input_root, &args.output.join("current"), &current)?;
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
