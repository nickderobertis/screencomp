//! Typed application errors and their stable process exit codes.
//!
//! Domain code stays infallible; everything that can fail is I/O or
//! configuration at a boundary, so a single [`AppError`] enum is sufficient.
//! This crate deliberately avoids `anyhow`/`miette` so callers can match on
//! error variants and so exit codes stay an explicit, tested mapping.

use camino::Utf8PathBuf;

use crate::config::ConfigError;

/// Errors surfaced at the process boundary.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    /// A path required to be an existing directory was missing or not a directory.
    #[error("not a directory: {path}")]
    NotADirectory {
        /// The offending path.
        path: Utf8PathBuf,
    },

    /// A filesystem operation failed.
    #[error("{context}")]
    Io {
        /// Human-readable description of the operation that failed.
        context: String,
        /// Underlying I/O error.
        #[source]
        source: std::io::Error,
    },

    /// An entry violated the `<project>/<name>.png` directory convention.
    #[error("invalid screenshot layout at {path}: {reason}")]
    InvalidLayout {
        /// The offending path.
        path: Utf8PathBuf,
        /// Why it was rejected.
        reason: String,
    },

    /// Configuration could not be loaded or failed validation.
    #[error(transparent)]
    Config(#[from] ConfigError),
}

impl AppError {
    /// Stable process exit code for this error.
    ///
    /// All application errors map to `1`; clap reserves `2` for usage errors and
    /// `classify --exit-code` uses `3` for a successful run that found changes.
    #[must_use]
    pub fn exit_code(&self) -> i32 {
        1
    }

    /// Construct an [`AppError::Io`] with operation context.
    pub(crate) fn io(context: impl Into<String>, source: std::io::Error) -> Self {
        Self::Io {
            context: context.into(),
            source,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_code_is_stable() {
        let not_dir = AppError::NotADirectory {
            path: Utf8PathBuf::from("x"),
        };
        assert_eq!(not_dir.exit_code(), 1);
        assert!(not_dir.to_string().contains("not a directory"));
    }

    #[test]
    fn io_carries_context_and_source() {
        let err = AppError::io("reading thing", std::io::Error::other("boom"));
        assert_eq!(err.to_string(), "reading thing");
        assert!(std::error::Error::source(&err).is_some());
        assert_eq!(err.exit_code(), 1);
    }
}
