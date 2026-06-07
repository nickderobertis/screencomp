//! Render a screenshot tree's digests as a stable text manifest.
//!
//! A manifest is the minimal baseline needed to classify a later capture: since
//! comparison is by content digest, the baseline pixels are unnecessary — only
//! the `(project, name) -> digest` mapping is. Committing this tiny text file
//! instead of the PNGs keeps a consuming repository free of binary churn while
//! preserving an exact, reviewable record of what each shot hashed to.
//!
//! The format is one `sha256sum`-style line per shot, sorted by `(project,
//! name)`:
//!
//! ```text
//! <hex-digest>  <project>/<name>.png
//! ```

use super::snapshot::Snapshot;

/// Render `snapshot` as a digest manifest (see module docs for the format).
///
/// Lines are emitted in `(project, name)` order, so the output is byte-stable
/// and diffs cleanly between runs.
pub(crate) fn render_manifest(snapshot: &Snapshot) -> String {
    let mut out = String::new();
    for (key, digest) in snapshot.iter() {
        out.push_str(digest);
        out.push_str("  ");
        out.push_str(&key.project);
        out.push('/');
        out.push_str(&key.name);
        out.push_str(".png\n");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::snapshot::ShotKey;

    fn key(project: &str, name: &str) -> ShotKey {
        ShotKey {
            project: project.to_owned(),
            name: name.to_owned(),
        }
    }

    #[test]
    fn renders_sorted_sha256sum_style_lines() {
        let mut snap = Snapshot::new();
        // Insert out of order to prove the render sorts by (project, name).
        snap.insert(key("desktop", "home"), "aa".to_owned());
        snap.insert(key("desktop", "about"), "bb".to_owned());
        snap.insert(key("mobile", "home"), "cc".to_owned());

        assert_eq!(
            render_manifest(&snap),
            "bb  desktop/about.png\naa  desktop/home.png\ncc  mobile/home.png\n"
        );
    }

    #[test]
    fn empty_snapshot_renders_empty() {
        assert_eq!(render_manifest(&Snapshot::new()), "");
    }
}
