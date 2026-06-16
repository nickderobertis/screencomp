//! Resolve the optional CPU-architecture dimension of a screenshot tree.
//!
//! Identical UI rendered on a different CPU architecture produces byte-different
//! PNGs (font hinting, rasterization), so a content-hash comparison is only
//! meaningful *within* one architecture. Captures always run in a Linux
//! container (see the crate docs), so the operating system never varies between
//! a developer and CI — the only dimension that does is the CPU arch. When an
//! arch is in play (an explicit `--arch`, or the project's committed
//! `[capture].arches`), a command scopes a screenshot root to a single `<arch>`
//! subtree — `<root>/<arch>/<project>/<name>.png` — so each arch is only ever
//! compared against its own baseline.
//!
//! Resolution is pure host introspection: it reads no filesystem and no
//! environment variables. The sole `auto` input maps to the host this binary
//! runs on. Whether the resolved subtree actually exists is decided later, by
//! the `io` layer that reads it.

use camino::{Utf8Path, Utf8PathBuf};

/// Sentinel `--arch` value that resolves to the host's own architecture.
pub(crate) const AUTO: &str = "auto";

/// Canonical CPU architecture of the host this binary runs on, e.g. `x86_64` or
/// `arm64`.
///
/// Derived from the compile-time target so it always names the binary that is
/// actually running, then normalized to the spellings used in screenshot trees
/// and container tags (`aarch64` → `arm64`). This is what `--arch auto` and a
/// host-defaulted command resolve to.
pub(crate) fn host_arch() -> String {
    canonical(std::env::consts::ARCH).to_owned()
}

/// Scope `root` to the requested arch subtree.
///
/// `None` leaves `root` untouched (no arch layer); `Some("auto")` resolves to
/// [`host_arch`]; any other value is used verbatim as the subtree name. The
/// returned path is not checked for existence here — that is the reader's job.
pub(crate) fn scope(root: &Utf8Path, arch: Option<&str>) -> Utf8PathBuf {
    match arch {
        None => root.to_owned(),
        Some(spec) => root.join(resolve(spec)),
    }
}

/// Resolve an arch spec to a concrete value (`auto` → [`host_arch`]).
///
/// Used both to build a scoped path ([`scope`]) and to surface the resolved
/// subtree name to the user (`doctor`).
pub(crate) fn resolve(spec: &str) -> String {
    if spec == AUTO {
        host_arch()
    } else {
        spec.to_owned()
    }
}

/// Normalize a CPU architecture to the screenshot-tree spelling
/// (`aarch64` → `arm64`); unknown values pass through unchanged so a new arch is
/// usable without a code change.
pub(crate) fn canonical(arch: &str) -> &str {
    match arch {
        "aarch64" | "arm64" => "arm64",
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_arch_is_nonempty() {
        assert!(!host_arch().is_empty());
    }

    #[test]
    fn arch_is_normalized_with_passthrough() {
        assert_eq!(canonical("aarch64"), "arm64");
        assert_eq!(canonical("arm64"), "arm64");
        assert_eq!(canonical("x86_64"), "x86_64");
        assert_eq!(canonical("riscv64"), "riscv64");
    }

    #[test]
    fn scope_without_arch_is_identity() {
        let root = Utf8Path::new("shots/baseline");
        assert_eq!(scope(root, None), root);
    }

    #[test]
    fn scope_appends_explicit_arch() {
        let root = Utf8Path::new("shots/baseline");
        assert_eq!(scope(root, Some("x86_64")), root.join("x86_64"));
    }

    #[test]
    fn scope_auto_uses_host_arch() {
        let root = Utf8Path::new("shots/baseline");
        assert_eq!(scope(root, Some(AUTO)), root.join(host_arch()));
    }

    #[test]
    fn resolve_passes_through_explicit() {
        assert_eq!(resolve("arm64"), "arm64");
        assert_eq!(resolve(AUTO), host_arch());
    }
}
