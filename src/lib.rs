//! `screencomp` — deterministic screenshot tooling for the visual-docs framework.
//!
//! The library exposes pure-by-default operations over a capture described by a
//! `captures.json` index (each shot's toggles, content hash, and image path),
//! optionally scoped under a `<root>/<arch>/` layer:
//!
//! - **classify** a current capture against a baseline (added/changed/removed/unchanged),
//! - **gallery** render a static HTML index of a capture, with user-defined toggle
//!   controls (theme, viewport, …) so one screen is one card you toggle through,
//! - **comment** render the sticky pull-request comment body for a classification,
//! - **manifest** write a capture's digests as a committable, image-free baseline,
//! - **verify** assert two captures of one build are byte-identical (the
//!   reproducibility gate),
//! - **doctor** preflight a capture's arch subtree and `captures.json` index,
//! - **arches** print the project's configured `[capture].arches` (drives the CI matrix),
//! - **scope** match a changed-path list against the `[guard].paths` globs, so
//!   the optional local pre-push guard re-captures only when it should,
//! - **init** scaffold a visual-docs setup (config, CI workflow, `.gitignore`).
//!
//! Core logic in `domain` is free of I/O; filesystem access is confined to `io`;
//! argument parsing lives in [`cli`]. The single entrypoint is [`run`], which
//! writes user-facing output to a caller-supplied writer so it can be exercised
//! in-process by tests as well as from the binary.

pub mod cli;

mod commands;
mod config;
mod domain;
mod errors;
mod io;

pub use cli::Cli;
pub use config::ConfigError;
pub use errors::AppError;

use std::io::Write;

use cli::Command;

/// Execute a parsed CLI invocation, writing user-facing output to `out`.
///
/// On success returns the intended process exit code: `0` normally, or `3` when
/// `classify --exit-code`/`doctor --exit-code` flag a problem, `verify` finds
/// the two captures diverge, or `scope --exit-code` matches a relevant path.
/// Errors and their causes are the
/// caller's responsibility to render (the binary prints them to stderr); this
/// function never writes to stderr.
///
/// # Errors
///
/// Returns [`AppError`] when inputs cannot be read, violate the directory
/// convention, or when configuration is invalid.
pub fn run(cli: Cli, out: &mut dyn Write) -> Result<i32, AppError> {
    // Load configuration once at the boundary so every command resolves arches,
    // comment, and guard settings from the same source.
    let config = commands::load_config(cli.config.as_deref())?;
    let ctx = commands::Ctx {
        quiet: cli.quiet,
        config,
    };
    match cli.command {
        Command::Classify(args) => commands::classify::run(&args, &ctx, out),
        Command::Gallery(args) => commands::gallery::run(&args, &ctx, out),
        Command::Comment(args) => commands::comment::run(&args, &ctx, out),
        Command::Manifest(args) => commands::manifest::run(&args, &ctx, out),
        Command::Verify(args) => commands::verify::run(&args, &ctx, out),
        Command::Doctor(args) => commands::doctor::run(&args, &ctx, out),
        Command::Arches(args) => commands::arches::run(&args, &ctx, out),
        Command::Scope(args) => commands::scope::run(&args, &ctx, out),
        Command::Init(args) => commands::init::run(&args, &ctx, out),
    }
}
