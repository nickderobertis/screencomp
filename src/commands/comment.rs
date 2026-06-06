//! `screencomp comment` — render the sticky pull-request comment body.

use std::io::Write;

use super::{Ctx, write_err};
use crate::cli::CommentArgs;
use crate::config::{self, CONFIG_ENV};
use crate::domain::classify::classify;
use crate::domain::comment::render_markdown;
use crate::errors::AppError;
use crate::io::fs;

pub(crate) fn run(args: &CommentArgs, ctx: &Ctx, out: &mut dyn Write) -> Result<i32, AppError> {
    // Environment is read once, here at the boundary.
    let env = std::env::var(CONFIG_ENV).ok();
    let cfg = config::load(args.config.as_deref(), env)?;

    let baseline = fs::discover(&args.baseline)?;
    let current = fs::discover(&args.current)?;
    let classification = classify(&baseline, &current);

    let markdown = render_markdown(
        &classification,
        &cfg.comment.title,
        &cfg.comment.marker,
        cfg.comment.show_unchanged,
        args.gallery_url.as_deref(),
    );

    match &args.output {
        Some(path) => {
            fs::write_string(path, &markdown)?;
            if !ctx.quiet {
                writeln!(out, "wrote {path}").map_err(write_err)?;
            }
        }
        None => write!(out, "{markdown}").map_err(write_err)?,
    }
    Ok(0)
}
