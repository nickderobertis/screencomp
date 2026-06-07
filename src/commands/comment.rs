//! `screencomp comment` — render the sticky pull-request comment body.

use std::io::Write;

use super::{Ctx, baseline_snapshot, platform, write_err};
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

    let plat = args.platform.as_deref();
    let baseline = baseline_snapshot(
        args.baseline.as_deref(),
        args.baseline_manifest.as_deref(),
        plat,
    )?;
    let current = fs::discover(&platform::scope(&args.current, plat))?;
    let classification = classify(&baseline, &current);

    // CLI flags override the configured values when present.
    let embed_limit = args.embed_limit.unwrap_or(cfg.comment.embed_limit);
    let title = args.title.as_deref().unwrap_or(&cfg.comment.title);
    let marker = args.marker.as_deref().unwrap_or(&cfg.comment.marker);

    let markdown = render_markdown(
        &classification,
        title,
        marker,
        cfg.comment.show_unchanged,
        args.gallery_url.as_deref(),
        embed_limit,
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
