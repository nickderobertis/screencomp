//! Helpers shared by the `integration` and `e2e` suites.
//!
//! Not a test target of its own (a subdirectory of `tests/` is not compiled as
//! one) — both suites pull it in with `mod common;`, so the subprocess
//! discipline below has a single copy rather than one per suite.

use std::ffi::OsStr;
use std::process::Command;
use std::sync::OnceLock;

/// The variables Git treats as repository-local, asked of the installed Git
/// (`git rev-parse --local-env-vars`) rather than restated here, so there is no
/// second copy to drift. Empty when Git is missing — which is also when no
/// spawned command can be misled by them.
fn repository_env() -> &'static [String] {
    static NAMES: OnceLock<Vec<String>> = OnceLock::new();
    NAMES.get_or_init(|| {
        Command::new("git")
            .args(["rev-parse", "--local-env-vars"])
            .output()
            .ok()
            .filter(|out| out.status.success())
            .map(|out| {
                String::from_utf8_lossy(&out.stdout)
                    .lines()
                    .map(str::trim)
                    .filter(|name| !name.is_empty())
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default()
    })
}

/// A subprocess with Git's repository-locating environment cleared, so a `git`
/// it runs resolves the repository from its working directory alone.
///
/// Spawn everything through this, never [`Command::new`] directly. The pre-push
/// hook runs both suites and Git exports `GIT_DIR` to a hook it runs, so a `git`
/// that inherited it — or one of the shipped shell scripts these tests execute —
/// acts on the repository being pushed instead of the temporary directory under
/// test: staging every tracked file's removal, committing it, renaming the
/// branch.
pub fn command(program: impl AsRef<OsStr>) -> Command {
    let mut command = Command::new(program);
    for name in repository_env() {
        command.env_remove(name);
    }
    command
}
