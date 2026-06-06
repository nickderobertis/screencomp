//! Binary entrypoint.
//!
//! `main` is intentionally thin: it parses arguments, delegates to
//! [`screencomp::run`], and maps the typed result to a process exit code.
//! All behavior lives in the library so it is testable without a subprocess.

use std::io::{self, Write as _};
use std::process::ExitCode;

use clap::Parser as _;
use screencomp::{Cli, run};

fn main() -> ExitCode {
    // `parse` handles `--help`/`--version` and usage errors itself, exiting with
    // the conventional codes (0 for help/version, 2 for bad usage).
    let cli = Cli::parse();

    let mut stdout = io::stdout().lock();
    match run(cli, &mut stdout) {
        Ok(code) => {
            let _ = stdout.flush();
            ExitCode::from(exit_byte(code))
        }
        Err(err) => {
            let _ = stdout.flush();
            report(&err);
            ExitCode::from(exit_byte(err.exit_code()))
        }
    }
}

/// Print an error and its source chain to stderr, keeping stdout machine-clean.
fn report(err: &screencomp::AppError) {
    eprintln!("error: {err}");
    let mut source = std::error::Error::source(err);
    while let Some(cause) = source {
        eprintln!("  caused by: {cause}");
        source = cause.source();
    }
}

/// Clamp an `i32` exit code into the `u8` range expected by [`ExitCode`].
fn exit_byte(code: i32) -> u8 {
    u8::try_from(code).unwrap_or(1)
}
