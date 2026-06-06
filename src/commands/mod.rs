//! Command handlers: orchestrate I/O ([`crate::io`]) and pure logic
//! ([`crate::domain`]) and produce user-facing output.

pub(crate) mod classify;
pub(crate) mod comment;
pub(crate) mod gallery;
pub(crate) mod platform;

use crate::errors::AppError;

/// Shared invocation context for command handlers.
pub(crate) struct Ctx {
    /// Suppress non-essential human output.
    pub(crate) quiet: bool,
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
