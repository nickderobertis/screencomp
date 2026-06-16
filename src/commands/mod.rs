//! Command handlers: orchestrate I/O ([`crate::io`]) and pure logic
//! ([`crate::domain`]) and produce user-facing output.

pub(crate) mod arch;
pub(crate) mod arches;
pub(crate) mod classify;
pub(crate) mod comment;
pub(crate) mod doctor;
pub(crate) mod gallery;
pub(crate) mod init;
pub(crate) mod manifest;
pub(crate) mod scope;
pub(crate) mod verify;

use camino::{Utf8Path, Utf8PathBuf};

use crate::config::{self, Config, ConfigError};
use crate::domain::layout::LayoutScan;
use crate::domain::snapshot::Snapshot;
use crate::errors::AppError;
use crate::io::fs;

/// Shared invocation context for command handlers.
///
/// Configuration is loaded once at the [`run`](crate::run) boundary and held
/// here so every command resolves `[capture].arches`, `[comment]`, and
/// `[guard]` from the same source without re-reading the file.
pub(crate) struct Ctx {
    /// Suppress non-essential human output.
    pub(crate) quiet: bool,
    /// Resolved configuration (defaults when no `screencomp.toml` is found).
    pub(crate) config: Config,
}

/// Resolve which arch subtree a command should scope to, honoring an explicit
/// `--arch` over the committed `[capture].arches`.
///
/// Precedence:
/// - an explicit `--arch` value wins and is trusted verbatim (`auto` → host),
/// - else, with `arches` configured, default to the host arch but require it to
///   be one of the configured arches (each arch owns a baseline and a CI lane),
/// - else (no `--arch`, no configured arches) return `None`: no arch layer, so
///   the root is treated as project-level (ad-hoc use without a config file).
pub(crate) fn resolve_arch(
    explicit: Option<&str>,
    arches: &[String],
) -> Result<Option<String>, AppError> {
    if let Some(spec) = explicit {
        return Ok(Some(arch::resolve(spec)));
    }
    if arches.is_empty() {
        return Ok(None);
    }
    let host = arch::host_arch();
    if arches.iter().any(|a| arch::canonical(a) == host) {
        return Ok(Some(host));
    }
    let configured = arches.join(", ");
    let mut all: Vec<String> = arches.iter().map(|a| format!("\"{a}\"")).collect();
    all.push(format!("\"{host}\""));
    Err(AppError::UnsupportedArch {
        host,
        configured,
        suggested: all.join(", "),
    })
}

/// Scope `root` to `arch` and walk it into a [`Snapshot`].
///
/// On a missing arch subtree the bare "not a directory" error is enriched with a
/// layout hint (see [`hint_missing_subtree`]), since that is the error a
/// first-time integrator is most likely to misread.
pub(crate) fn discover_scoped(root: &Utf8Path, arch: Option<&str>) -> Result<Snapshot, AppError> {
    let scoped = arch::scope(root, arch);
    fs::discover(&scoped).map_err(|e| hint_missing_subtree(e, root, arch))
}

/// Scope `root` to `arch` and scan its layout, with the same hint as
/// [`discover_scoped`] on a missing subtree.
pub(crate) fn scan_scoped(root: &Utf8Path, arch: Option<&str>) -> Result<LayoutScan, AppError> {
    let scoped = arch::scope(root, arch);
    fs::scan_layout(&scoped).map_err(|e| hint_missing_subtree(e, root, arch))
}

/// Turn a bare [`AppError::NotADirectory`] for a missing arch subtree into an
/// [`AppError::InvalidLayout`] that explains the arch layer.
///
/// An arch adds a `<root>/<arch>/` layer above `<project>/<name>.png`. When that
/// subtree is absent but `root` itself exists, the usual cause is a wrong arch
/// (e.g. `arm64` vs `x86_64`) or a capture written without the arch segment —
/// both of which the bare error hides. The hint names the host arch and what the
/// root actually holds so the fix is obvious. The error is returned untouched
/// when no arch was requested or the root is also missing (a genuinely absent
/// tree, not a layout mistake).
fn hint_missing_subtree(err: AppError, root: &Utf8Path, arch_spec: Option<&str>) -> AppError {
    let (Some(spec), AppError::NotADirectory { path }) = (arch_spec, &err) else {
        return err;
    };
    if !root.is_dir() {
        return err;
    }
    let key = arch::resolve(spec);
    let host = arch::host_arch();

    let mut reason = format!(
        "expected the arch subtree {path} \
         (with --arch {key}, screencomp looks for {root}/{key}/<project>/<name>.png)"
    );
    match fs::scan_layout(root) {
        Ok(scan) if !scan.loose_pngs.is_empty() => reason.push_str(&format!(
            "; found {} loose .png file(s) directly under {root} \
             — move them into {root}/{key}/<project>/, or omit --arch",
            scan.loose_pngs.len()
        )),
        Ok(scan) if !scan.projects.is_empty() => {
            let names: Vec<&str> = scan.projects.iter().map(|p| p.name.as_str()).collect();
            reason.push_str(&format!(
                "; {root} contains [{}] instead — check the arch \
                 (this host is {host}), or omit --arch",
                names.join(", ")
            ));
        }
        _ => {}
    }
    AppError::InvalidLayout {
        path: path.clone(),
        reason,
    }
}

/// Resolve a baseline snapshot from either an image tree or a digest manifest.
///
/// Exactly one of `dir`/`manifest` is `Some` (clap's argument group enforces
/// this). A `dir` is scoped to `arch` and walked; a `manifest` is already
/// arch-specific, so `arch` does not apply to it.
pub(crate) fn baseline_snapshot(
    dir: Option<&Utf8Path>,
    manifest: Option<&Utf8Path>,
    arch: Option<&str>,
) -> Result<Snapshot, AppError> {
    match (dir, manifest) {
        (Some(dir), None) => discover_scoped(dir, arch),
        (None, Some(manifest)) => fs::read_manifest(manifest),
        _ => unreachable!("clap argument group guarantees exactly one baseline source"),
    }
}

/// Load configuration at a command boundary, reading the ambient bits here.
///
/// Reads `$SCREENCOMP_CONFIG` and, when neither `explicit` nor that env var is
/// set, auto-discovers `screencomp.toml` by walking up from the working
/// directory. Centralized so `comment` and `scope` resolve config identically.
pub(crate) fn load_config(explicit: Option<&Utf8Path>) -> Result<Config, ConfigError> {
    let env = std::env::var(config::CONFIG_ENV).ok();
    // Auto-discovery only kicks in with no explicit source, so an explicit choice
    // is never silently overridden by a stray file up the tree.
    let discovered = if explicit.is_none() && env.as_deref().unwrap_or("").is_empty() {
        std::env::current_dir()
            .ok()
            .and_then(|cwd| Utf8PathBuf::from_path_buf(cwd).ok())
            .and_then(|cwd| fs::find_up(&cwd, config::CONFIG_FILE))
    } else {
        None
    };
    config::load(explicit, env, discovered)
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

    #[test]
    fn resolve_arch_explicit_wins_and_resolves_auto() {
        assert_eq!(
            resolve_arch(Some("x86_64"), &[]).unwrap(),
            Some("x86_64".to_owned())
        );
        assert_eq!(
            resolve_arch(Some("auto"), &["sparc".to_owned()]).unwrap(),
            Some(arch::host_arch())
        );
    }

    #[test]
    fn resolve_arch_is_none_without_config() {
        assert_eq!(resolve_arch(None, &[]).unwrap(), None);
    }

    #[test]
    fn resolve_arch_defaults_to_host_when_listed() {
        let host = arch::host_arch();
        assert_eq!(resolve_arch(None, &[host.clone()]).unwrap(), Some(host));
    }

    #[test]
    fn resolve_arch_errors_when_host_not_listed() {
        // A list that cannot contain the real host arch.
        let err = resolve_arch(None, &["sparc64".to_owned()]).unwrap_err();
        let AppError::UnsupportedArch { suggested, .. } = &err else {
            panic!("expected UnsupportedArch, got {err:?}");
        };
        // The suggestion keeps the existing entry and appends the host arch.
        assert!(suggested.contains("\"sparc64\""), "{suggested}");
        assert!(suggested.contains(&format!("\"{}\"", arch::host_arch())), "{suggested}");
    }
}
