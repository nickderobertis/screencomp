//! `screencomp comment` — render the sticky pull-request comment body.

use std::io::Write;

use super::{Ctx, baseline_snapshot, discover_scoped, write_err};
use crate::cli::CommentArgs;
use crate::config::{self, CONFIG_ENV};
use crate::domain::classify::classify;
use crate::domain::comment::{ImageBases, render_markdown};
use crate::errors::AppError;
use crate::io::fs;

pub(crate) fn run(args: &CommentArgs, ctx: &Ctx, out: &mut dyn Write) -> Result<i32, AppError> {
    // Environment is read once, here at the boundary.
    let env = std::env::var(CONFIG_ENV).ok();
    let cfg = config::load(args.config.as_deref(), env)?;

    let plat = args.platform.as_deref();
    let manifest_mode = args.baseline_manifest.is_some();
    let baseline = baseline_snapshot(
        args.baseline.as_deref(),
        args.baseline_manifest.as_deref(),
        plat,
    )?;
    let current = discover_scoped(&args.current, plat)?;
    let classification = classify(&baseline, &current);

    // CLI flags override the configured values when present.
    let embed_limit = args.embed_limit.unwrap_or(cfg.comment.embed_limit);
    let title = args.title.as_deref().unwrap_or(&cfg.comment.title);
    let marker = args.marker.as_deref().unwrap_or(&cfg.comment.marker);

    // Resolve where the "Before"/"After" images are hosted. Explicit flags win;
    // otherwise `--gallery-url` derives them from the layout `gallery` writes,
    // which differs by baseline mode (see `image_bases`).
    let (before, after) = image_bases(args, manifest_mode);
    let markdown = render_markdown(
        &classification,
        title,
        marker,
        cfg.comment.show_unchanged,
        args.gallery_url.as_deref(),
        ImageBases {
            before: before.as_deref(),
            after: after.as_deref(),
        },
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

/// Resolve the `(before, after)` image bases for the inline previews.
///
/// `--baseline-url`/`--current-url` are used verbatim when set. Otherwise
/// `--gallery-url U` derives them from the layout `gallery` produces — which
/// depends on the baseline mode:
///
/// - image-tree baseline (`--baseline`): a diff gallery deployed at `U`, so
///   `U/baseline` and `U/current`;
/// - manifest baseline (`--baseline-manifest`): no committed baseline PNGs, so a
///   diff gallery is impossible. The current shots are a plain gallery at `U`,
///   and "Before" is left unhosted (no `<img>` that would 404) unless an explicit
///   `--baseline-url` points at a canonical gallery.
fn image_bases(args: &CommentArgs, manifest_mode: bool) -> (Option<String>, Option<String>) {
    let gallery = args.gallery_url.as_deref().map(trim_slash);

    let after = match args.current_url.as_deref() {
        Some(url) => Some(trim_slash(url)),
        None => gallery.as_ref().map(|u| {
            if manifest_mode {
                u.clone()
            } else {
                format!("{u}/current")
            }
        }),
    };

    let before = match args.baseline_url.as_deref() {
        Some(url) => Some(trim_slash(url)),
        // In manifest mode there is no baseline tree to derive from a gallery URL.
        None if manifest_mode => None,
        None => gallery.as_ref().map(|u| format!("{u}/baseline")),
    };

    (before, after)
}

/// Drop a single trailing slash so derived subpaths join cleanly.
fn trim_slash(url: &str) -> String {
    url.trim_end_matches('/').to_owned()
}
