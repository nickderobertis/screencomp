//! Resolve the optional platform dimension of a screenshot tree.
//!
//! Identical UI rendered on a different operating system or CPU architecture
//! produces byte-different PNGs (font hinting, rasterization), so a content-hash
//! comparison is only meaningful *within* one platform. When `--platform` is
//! given, a command scopes a screenshot root to a single `<os>-<arch>` subtree —
//! `<root>/<platform>/<project>/<name>.png` — so each platform is only ever
//! compared against its own baseline.
//!
//! Resolution is pure host introspection: it reads no filesystem and no
//! environment variables. The sole `auto` input maps to the platform this binary
//! was built for. Whether the resolved subtree actually exists is decided later,
//! by the `io` layer that reads it.

use camino::{Utf8Path, Utf8PathBuf};

/// Sentinel `--platform` value that resolves to the host's own key.
pub(crate) const AUTO: &str = "auto";

/// Canonical platform key for the host this binary runs on, e.g.
/// `linux-x86_64` or `macos-arm64`.
///
/// Derived from the compile-time target so it always names the binary that is
/// actually running, then normalized to the spellings used in screenshot trees
/// and container tags.
pub(crate) fn host_key() -> String {
    format!(
        "{}-{}",
        canonical_os(std::env::consts::OS),
        canonical_arch(std::env::consts::ARCH)
    )
}

/// Canonical platform key for a *Linux-container* capture on this host's arch,
/// e.g. `linux-x86_64` or `linux-arm64`.
///
/// Unlike [`host_key`], the OS is fixed to `linux`: the `init` scaffold renders
/// inside a Linux container regardless of the developer's host OS, so labeling a
/// capture by the host OS (`macos-arm64` on a Mac) would misname Linux pixels.
/// Only the arch — which the container inherits from the host for native speed —
/// varies. This is what `init --platform auto` resolves to.
pub(crate) fn host_container_key() -> String {
    format!("linux-{}", canonical_arch(std::env::consts::ARCH))
}

/// Scope `root` to the requested platform subtree.
///
/// `None` leaves `root` untouched (no platform layer); `Some("auto")` resolves
/// to [`host_key`]; any other value is used verbatim as the subtree name. The
/// returned path is not checked for existence here — that is the reader's job.
pub(crate) fn scope(root: &Utf8Path, platform: Option<&str>) -> Utf8PathBuf {
    match platform {
        None => root.to_owned(),
        Some(spec) => root.join(resolve(spec)),
    }
}

/// Resolve a `--platform` spec to a concrete key (`auto` → [`host_key`]).
///
/// Used both to build a scoped path ([`scope`]) and to surface the resolved
/// subtree name to the user (`doctor`).
pub(crate) fn resolve(spec: &str) -> String {
    if spec == AUTO {
        host_key()
    } else {
        spec.to_owned()
    }
}

/// Normalize an OS name to the screenshot-tree spelling.
///
/// Rust reports `linux`/`windows` directly and macOS as `macos`; the
/// target-triple `darwin` is folded onto `macos` for callers that pass it.
/// Unknown values pass through so a new platform is usable without a code change.
fn canonical_os(os: &str) -> &str {
    match os {
        "macos" | "darwin" => "macos",
        other => other,
    }
}

/// Normalize a CPU architecture to the screenshot-tree spelling
/// (`aarch64` → `arm64`); unknown values pass through unchanged.
fn canonical_arch(arch: &str) -> &str {
    match arch {
        "aarch64" | "arm64" => "arm64",
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_key_is_nonempty_os_dash_arch() {
        let key = host_key();
        let (os, arch) = key.split_once('-').expect("key has an os-arch shape");
        assert!(!os.is_empty() && !arch.is_empty(), "{key}");
    }

    #[test]
    fn arch_is_normalized_with_passthrough() {
        assert_eq!(canonical_arch("aarch64"), "arm64");
        assert_eq!(canonical_arch("arm64"), "arm64");
        assert_eq!(canonical_arch("x86_64"), "x86_64");
        assert_eq!(canonical_arch("riscv64"), "riscv64");
    }

    #[test]
    fn os_is_normalized_with_passthrough() {
        assert_eq!(canonical_os("macos"), "macos");
        assert_eq!(canonical_os("darwin"), "macos");
        assert_eq!(canonical_os("linux"), "linux");
        assert_eq!(canonical_os("windows"), "windows");
        assert_eq!(canonical_os("freebsd"), "freebsd");
    }

    #[test]
    fn container_key_is_linux_with_the_host_arch() {
        let key = host_container_key();
        // Always Linux (the scaffold captures in a container), arch from the host.
        assert_eq!(
            key,
            format!("linux-{}", canonical_arch(std::env::consts::ARCH))
        );
        assert!(key.starts_with("linux-"), "{key}");
    }

    #[test]
    fn scope_without_platform_is_identity() {
        let root = Utf8Path::new("shots/baseline");
        assert_eq!(scope(root, None), root);
    }

    #[test]
    fn scope_appends_explicit_key() {
        let root = Utf8Path::new("shots/baseline");
        assert_eq!(scope(root, Some("linux-x86_64")), root.join("linux-x86_64"));
    }

    #[test]
    fn scope_auto_uses_host_key() {
        let root = Utf8Path::new("shots/baseline");
        assert_eq!(scope(root, Some(AUTO)), root.join(host_key()));
    }
}
