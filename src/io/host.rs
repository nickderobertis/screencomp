//! Host-environment access: Git config and `PATH` lookups.
//!
//! Like the filesystem boundary in [`super::fs`], this is the *only* place the
//! crate shells out to Git or inspects `PATH`, so the rest of the code stays
//! free of process state. It backs `init --enable-hook` (a Git write) and
//! `doctor --env` (Git and tool-presence reads), both of which need to know
//! whether the local repository is actually wired to run the pre-push guard.

use std::process::Command;

use camino::Utf8Path;

/// Environment variables through which Git overrides where the repository is
/// (`git rev-parse --local-env-vars`). Git exports `GIT_DIR` (and, for some
/// hooks, `GIT_INDEX_FILE` and friends) to every hook it runs, and a child `git`
/// honours them over `-C <dir>` — so a `screencomp init --enable-hook` launched
/// from inside a hook would configure the hook's repository, not `dir`'s. Every
/// spawn here drops them so `-C <dir>` alone decides which repository is read or
/// written.
const REPOSITORY_ENV: &[&str] = &[
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_CONFIG",
    "GIT_CONFIG_PARAMETERS",
    "GIT_CONFIG_COUNT",
    "GIT_OBJECT_DIRECTORY",
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_IMPLICIT_WORK_TREE",
    "GIT_GRAFT_FILE",
    "GIT_INDEX_FILE",
    "GIT_NO_REPLACE_OBJECTS",
    "GIT_REPLACE_REF_BASE",
    "GIT_PREFIX",
    "GIT_SHALLOW_FILE",
    "GIT_COMMON_DIR",
];

/// `git -C <dir> …` with the repository-locating environment cleared, so the
/// command acts on `dir` regardless of what a surrounding hook exported.
fn git_in(dir: &Utf8Path) -> Command {
    let mut command = Command::new("git");
    command.args(["-C", dir.as_str()]);
    for var in REPOSITORY_ENV {
        command.env_remove(var);
    }
    command
}

/// Outcome of trying to enable the committed hooks directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum HookEnable {
    /// `core.hooksPath` was set successfully.
    Set,
    /// `git` is not installed, so nothing was changed.
    GitUnavailable,
    /// `git` ran but refused (e.g. `dir` is not a Git repository); carries the
    /// trimmed stderr so the caller can explain why.
    Failed(String),
}

/// Point Git at `dir`'s committed hooks directory via
/// `git -C <dir> config core.hooksPath <value>`.
///
/// Best-effort by design: a missing `git` or a non-repository directory is
/// reported, not raised, so `init`'s scaffold still succeeds and the user gets a
/// clear next step instead of a failed command.
pub(crate) fn set_hooks_path(dir: &Utf8Path, value: &str) -> HookEnable {
    let output = git_in(dir)
        .args(["config", "core.hooksPath", value])
        .output();
    match output {
        Ok(out) if out.status.success() => HookEnable::Set,
        Ok(out) => HookEnable::Failed(String::from_utf8_lossy(&out.stderr).trim().to_owned()),
        Err(_) => HookEnable::GitUnavailable,
    }
}

/// Read the effective `core.hooksPath` for the repository at `dir`, if any.
///
/// Returns `None` when Git is unavailable, `dir` is not a repository, or the
/// setting is unset — all of which mean "the committed guard is not wired up"
/// for preflight purposes. The value is whatever Git resolves (local, global, or
/// system config), so a globally enabled hook is detected too.
pub(crate) fn hooks_path(dir: &Utf8Path) -> Option<String> {
    let output = git_in(dir)
        .args(["config", "--get", "core.hooksPath"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    (!value.is_empty()).then_some(value)
}

/// Whether `name` resolves to an existing file on `PATH`.
///
/// A cheap presence check for a preflight advisory (e.g. "is Docker installed?"),
/// not a guarantee the tool runs — it never executes the binary.
pub(crate) fn command_exists(name: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    // `.exe` on Windows, empty elsewhere — so a `PATH` lookup also finds `docker.exe`.
    let exe = std::env::consts::EXE_SUFFIX;
    std::env::split_paths(&path).any(|dir| {
        dir.join(name).is_file() || (!exe.is_empty() && dir.join(format!("{name}{exe}")).is_file())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hooks_path_is_none_outside_a_repo() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = Utf8Path::from_path(tmp.path()).unwrap();
        // A bare temp dir is not a Git repository, so there is no hooksPath.
        assert_eq!(hooks_path(dir), None);
    }

    #[test]
    fn set_then_read_hooks_path_roundtrips_in_a_repo() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = Utf8Path::from_path(tmp.path()).unwrap();
        // Skip silently if git is not installed in the test environment.
        if Command::new("git").arg("--version").output().is_err() {
            return;
        }
        assert!(git_in(dir).args(["init", "-q"]).status().unwrap().success());
        assert_eq!(set_hooks_path(dir, ".githooks"), HookEnable::Set);
        assert_eq!(hooks_path(dir), Some(".githooks".to_owned()));
    }

    #[test]
    fn command_exists_finds_a_ubiquitous_tool_and_rejects_nonsense() {
        // `sh` is on PATH on every platform this is tested on; the random name is not.
        assert!(command_exists("sh") || command_exists("cmd"));
        assert!(!command_exists("screencomp-no-such-binary-xyzzy"));
    }
}
