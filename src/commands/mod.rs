//! Command handlers: orchestrate I/O ([`crate::io`]) and pure logic
//! ([`crate::domain`]) and produce user-facing output.

pub(crate) mod classify;
pub(crate) mod comment;
pub(crate) mod doctor;
pub(crate) mod gallery;
pub(crate) mod manifest;
pub(crate) mod platform;
pub(crate) mod scope;
pub(crate) mod verify;

use camino::Utf8Path;

use crate::domain::layout::LayoutScan;
use crate::domain::snapshot::Snapshot;
use crate::errors::AppError;
use crate::io::fs;

/// Shared invocation context for command handlers.
pub(crate) struct Ctx {
    /// Suppress non-essential human output.
    pub(crate) quiet: bool,
}

/// Scope `root` to `platform` and walk it into a [`Snapshot`].
///
/// On a missing `--platform` subtree the bare "not a directory" error is
/// enriched with a layout hint (see [`hint_missing_subtree`]), since that is the
/// error a first-time integrator is most likely to misread.
pub(crate) fn discover_scoped(
    root: &Utf8Path,
    platform: Option<&str>,
) -> Result<Snapshot, AppError> {
    let scoped = platform::scope(root, platform);
    fs::discover(&scoped).map_err(|e| hint_missing_subtree(e, root, platform))
}

/// Scope `root` to `platform` and scan its layout, with the same hint as
/// [`discover_scoped`] on a missing subtree.
pub(crate) fn scan_scoped(root: &Utf8Path, platform: Option<&str>) -> Result<LayoutScan, AppError> {
    let scoped = platform::scope(root, platform);
    fs::scan_layout(&scoped).map_err(|e| hint_missing_subtree(e, root, platform))
}

/// Turn a bare [`AppError::NotADirectory`] for a missing `--platform` subtree
/// into an [`AppError::InvalidLayout`] that explains the platform layer.
///
/// `--platform` adds a `<root>/<key>/` layer above `<project>/<name>.png`. When
/// that subtree is absent but `root` itself exists, the usual cause is a wrong
/// platform key (e.g. `linux-arm64` vs `linux-x86_64`) or a capture written
/// without the platform segment — both of which the bare error hides. The hint
/// names the host key and what the root actually holds so the fix is obvious. The
/// error is returned untouched when no platform was requested or the root is also
/// missing (a genuinely absent tree, not a layout mistake).
fn hint_missing_subtree(err: AppError, root: &Utf8Path, platform: Option<&str>) -> AppError {
    let (Some(spec), AppError::NotADirectory { path }) = (platform, &err) else {
        return err;
    };
    if !root.is_dir() {
        return err;
    }
    let key = platform::resolve(spec);
    let host = platform::host_key();

    let mut reason = format!(
        "expected the platform subtree {path} \
         (with --platform {key}, screencomp looks for {root}/{key}/<project>/<name>.png)"
    );
    match fs::scan_layout(root) {
        Ok(scan) if !scan.loose_pngs.is_empty() => reason.push_str(&format!(
            "; found {} loose .png file(s) directly under {root} \
             — move them into {root}/{key}/<project>/, or omit --platform",
            scan.loose_pngs.len()
        )),
        Ok(scan) if !scan.projects.is_empty() => {
            let names: Vec<&str> = scan.projects.iter().map(|p| p.name.as_str()).collect();
            reason.push_str(&format!(
                "; {root} contains [{}] instead — check the platform key \
                 (this host is {host}), or omit --platform",
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
/// this). A `dir` is scoped to `platform` and walked; a `manifest` is already
/// platform-specific, so `platform` does not apply to it.
pub(crate) fn baseline_snapshot(
    dir: Option<&Utf8Path>,
    manifest: Option<&Utf8Path>,
    platform: Option<&str>,
) -> Result<Snapshot, AppError> {
    match (dir, manifest) {
        (Some(dir), None) => discover_scoped(dir, platform),
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
