//! `screencomp` — deterministic screenshot tooling for the visual-docs framework.
//!
//! The library exposes three pure-by-default operations over a screenshot tree
//! laid out as `<root>/<project>/<name>.png`:
//!
//! - **classify** a current capture against a baseline (added/changed/removed/unchanged),
//! - **gallery** render a static HTML index of a capture,
//! - **comment** render the sticky pull-request comment body for a classification,
//! - **manifest** write a tree's digests as a committable, image-free baseline.
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
/// `classify --exit-code` detects differences. Errors and their causes are the
/// caller's responsibility to render (the binary prints them to stderr); this
/// function never writes to stderr.
///
/// # Errors
///
/// Returns [`AppError`] when inputs cannot be read, violate the directory
/// convention, or when configuration is invalid.
pub fn run(cli: Cli, out: &mut dyn Write) -> Result<i32, AppError> {
    let ctx = commands::Ctx { quiet: cli.quiet };
    match cli.command {
        Command::Classify(args) => commands::classify::run(&args, &ctx, out),
        Command::Gallery(args) => commands::gallery::run(&args, &ctx, out),
        Command::Comment(args) => commands::comment::run(&args, &ctx, out),
        Command::Manifest(args) => commands::manifest::run(&args, &ctx, out),
    }
}
