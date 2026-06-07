//! Command handlers: orchestrate I/O ([`crate::io`]) and pure logic
//! ([`crate::domain`]) and produce user-facing output.

pub(crate) mod classify;
pub(crate) mod comment;
pub(crate) mod gallery;
pub(crate) mod manifest;
pub(crate) mod platform;

use camino::Utf8Path;

use crate::domain::snapshot::Snapshot;
use crate::errors::AppError;
use crate::io::fs;

/// Shared invocation context for command handlers.
pub(crate) struct Ctx {
    /// Suppress non-essential human output.
    pub(crate) quiet: bool,
}

/// Resolve a baseline snapshot from either an image tree or a digest manifest.
///
/// Exactly one of `dir`/`manifest` is `Some` (clap's argument group enforces
/// this). A `dir` is scoped to `platform` and walked; a `manifest` is already
/// platform-specific, so `platform` does not apply to it.
pub(crate) fn baseline_snapshot(
    dir: Option<&Utf8Path>,
    manifest: Option<&Utf8Path>,
    platform: Option<&str>,
) -> Result<Snapshot, AppError> {
    match (dir, manifest) {
        (Some(dir), None) => fs::discover(&platform::scope(dir, platform)),
        (None, Some(manifest)) => fs::read_manifest(manifest),
        _ => unreachable!("clap argument group guarantees exactly one baseline source"),
    }
}

/// Map a writer failure into an [`AppError`].
pub(crate) fn write_err(source: std::io::Error) -> AppError {
    AppError::io("writing output", source)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_err_wraps_as_io() {
        let err = write_err(std::io::Error::other("disk full"));
        assert!(matches!(err, AppError::Io { .. }));
        assert_eq!(err.exit_code(), 1);
    }
}
