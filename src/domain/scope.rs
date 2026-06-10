//! Pure glob matching for the pre-push guard's "are screenshot-relevant files
//! changed?" check.
//!
//! The guard must decide, from a list of changed paths, whether any is relevant
//! enough to justify a (slow, Docker-backed) re-capture. Doing that with shell
//! globbing is fragile; instead the hook pipes the change set into `screencomp
//! scope`, which delegates here. This module only matches strings against globs
//! — it reads no filesystem, environment, or git state — so it is deterministic,
//! offline, and trivially unit-testable.
//!
//! Supported syntax (a deliberately small, path-oriented subset):
//!
//! - `*` matches any run of characters except the path separator `/`,
//! - `**` matches any run of characters *including* `/` (whole path segments),
//!   and `**/` may also match zero segments so `a/**/b` matches `a/b`,
//! - `?` matches exactly one character other than `/`,
//! - every other character matches itself literally.
//!
//! Matching is anchored: the whole `path` must be consumed by the whole
//! `pattern`. Globs are matched against the path verbatim, so they should be
//! written relative to the repository root, exactly as `git diff --name-only`
//! emits them.

/// Whether any of `patterns` matches `path`.
pub(crate) fn any_match(patterns: &[String], path: &str) -> bool {
    patterns.iter().any(|pattern| matches(pattern, path))
}

/// Whether `pattern` matches the whole of `path` under the syntax in the module
/// docs.
pub(crate) fn matches(pattern: &str, path: &str) -> bool {
    glob_match(pattern.as_bytes(), path.as_bytes())
}

/// Anchored, backtracking glob match over byte slices.
///
/// Operating on bytes keeps the matcher allocation-free; UTF-8 is matched
/// literally, which is correct because every metacharacter (`*`, `?`, `/`) is
/// ASCII and so never splits a multi-byte sequence.
fn glob_match(pattern: &[u8], path: &[u8]) -> bool {
    match pattern.first() {
        None => path.is_empty(),
        Some(b'*') if pattern.get(1) == Some(&b'*') => {
            // `**` spans path separators. After it (skipping one optional `/`)
            // the rest may match zero segments, so `**/x` matches `x`.
            if glob_match(&pattern[2..], path) {
                return true;
            }
            if pattern.get(2) == Some(&b'/') && glob_match(&pattern[3..], path) {
                return true;
            }
            // Otherwise consume one byte (of any kind) and retry.
            !path.is_empty() && glob_match(pattern, &path[1..])
        }
        Some(b'*') => {
            // `*` matches a run of non-`/` bytes. Try zero first, then consume.
            if glob_match(&pattern[1..], path) {
                return true;
            }
            matches!(path.first(), Some(&c) if c != b'/') && glob_match(pattern, &path[1..])
        }
        Some(b'?') => {
            matches!(path.first(), Some(&c) if c != b'/') && glob_match(&pattern[1..], &path[1..])
        }
        Some(&lit) => path.first() == Some(&lit) && glob_match(&pattern[1..], &path[1..]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literal_match_is_anchored() {
        assert!(matches("src/lib.rs", "src/lib.rs"));
        assert!(!matches("src/lib.rs", "src/lib.rs.bak"));
        assert!(!matches("src/lib.rs", "a/src/lib.rs"));
    }

    #[test]
    fn single_star_does_not_cross_slash() {
        assert!(matches("*.png", "home.png"));
        assert!(matches("src/*.rs", "src/lib.rs"));
        assert!(!matches("*.png", "desktop/home.png"));
        assert!(!matches("src/*.rs", "src/ui/lib.rs"));
    }

    #[test]
    fn double_star_crosses_slashes() {
        assert!(matches("shots/**", "shots/current/desktop/home.png"));
        assert!(matches("**", "anything/at/all.txt"));
        assert!(matches("src/**/*.rs", "src/ui/widgets/button.rs"));
    }

    #[test]
    fn double_star_segment_matches_zero_dirs() {
        // `**/` collapses to nothing, so an enclosed `**` still matches when no
        // intermediate directory is present.
        assert!(matches("src/**/*.rs", "src/lib.rs"));
        assert!(matches("a/**/b", "a/b"));
        assert!(matches("a/**/b", "a/x/y/b"));
        assert!(matches("**/Cargo.toml", "Cargo.toml"));
        assert!(matches("**/Cargo.toml", "crates/inner/Cargo.toml"));
    }

    #[test]
    fn question_mark_matches_one_non_slash() {
        assert!(matches("v?.png", "v1.png"));
        assert!(!matches("v?.png", "v12.png"));
        assert!(!matches("a?b", "a/b"));
    }

    #[test]
    fn empty_pattern_only_matches_empty_path() {
        assert!(matches("", ""));
        assert!(!matches("", "x"));
        assert!(!matches("x", ""));
    }

    #[test]
    fn any_match_is_an_or_over_patterns() {
        let globs = vec!["src/**/*.rs".to_owned(), "playwright/**".to_owned()];
        assert!(any_match(&globs, "src/ui/button.rs"));
        assert!(any_match(&globs, "playwright/tests/home.spec.ts"));
        assert!(!any_match(&globs, "README.md"));
        // No patterns never matches.
        assert!(!any_match(&[], "src/lib.rs"));
    }
}
