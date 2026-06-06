//! In-memory model of a screenshot tree, content-addressed by digest.

use std::collections::BTreeMap;

/// Identifies a screenshot by its project (Playwright variant) and base name.
///
/// Ordering is `(project, name)`, which makes every derived listing
/// deterministic without an explicit sort at each use site.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct ShotKey {
    /// Project/variant directory name.
    pub(crate) project: String,
    /// Screenshot base name (file stem, without the `.png` extension).
    pub(crate) name: String,
}

/// A screenshot tree mapping each [`ShotKey`] to the hex digest of its PNG bytes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Snapshot {
    shots: BTreeMap<ShotKey, String>,
}

impl Snapshot {
    /// Create an empty snapshot.
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Record a screenshot's digest, overwriting any previous entry for the key.
    pub(crate) fn insert(&mut self, key: ShotKey, digest: String) {
        self.shots.insert(key, digest);
    }

    /// Digest for `key`, if present.
    pub(crate) fn get(&self, key: &ShotKey) -> Option<&str> {
        self.shots.get(key).map(String::as_str)
    }

    /// Keys in `(project, name)` order.
    pub(crate) fn keys(&self) -> impl Iterator<Item = &ShotKey> {
        self.shots.keys()
    }

    /// `(key, digest)` pairs in `(project, name)` order.
    pub(crate) fn iter(&self) -> impl Iterator<Item = (&ShotKey, &str)> {
        self.shots.iter().map(|(k, v)| (k, v.as_str()))
    }

    /// Whether the snapshot contains no screenshots.
    pub(crate) fn is_empty(&self) -> bool {
        self.shots.is_empty()
    }
}
