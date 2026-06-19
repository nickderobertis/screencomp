//! In-memory model of a capture, content-addressed by digest.
//!
//! A capture is a set of *shots*. Each shot is identified by a [`ShotKey`] — a
//! base `name` plus the set of *toggle* values that produced it (e.g.
//! `theme=dark`, `viewport=mobile`). The old fixed `project` directory dimension
//! is gone: a "project" is now just one toggle among any number the consumer
//! declares, so a single logical screenshot collapses into one gallery card with
//! toggle controls instead of one card per variant.
//!
//! The source of truth on disk is a `captures.json` index (see
//! [`crate::domain::index`]) that carries each shot's toggles, content hash, and
//! image path; nothing here walks a directory tree.

use std::collections::BTreeMap;

/// A shot's toggle assignment: dimension key → chosen value, e.g.
/// `{"theme": "dark", "viewport": "mobile"}`.
///
/// A [`BTreeMap`] so iteration (and thus every derived key/label/render) is in
/// dimension-key order and byte-stable without an explicit sort.
pub(crate) type Toggles = BTreeMap<String, String>;

/// Identifies a shot by its base name and the toggle values that produced it.
///
/// Ordering is `(name, toggles)`, which makes every derived listing
/// deterministic without an explicit sort at each use site.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct ShotKey {
    /// Screenshot base name (the logical screen, e.g. `home`).
    pub(crate) name: String,
    /// Toggle values that produced this shot (empty for a name with no toggles).
    pub(crate) toggles: Toggles,
}

impl ShotKey {
    /// A shot with no toggles.
    #[cfg(test)]
    pub(crate) fn bare(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            toggles: Toggles::new(),
        }
    }

    /// Build a key from a name and `(key, value)` toggle pairs.
    #[cfg(test)]
    pub(crate) fn with(name: &str, toggles: &[(&str, &str)]) -> Self {
        Self {
            name: name.to_owned(),
            toggles: toggles
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect(),
        }
    }

    /// Stable, human-readable label: the name, plus its toggles in key order when
    /// any are set — `home` or `home [theme=dark, viewport=mobile]`.
    pub(crate) fn label(&self) -> String {
        if self.toggles.is_empty() {
            self.name.clone()
        } else {
            let dims: Vec<String> = self
                .toggles
                .iter()
                .map(|(k, v)| format!("{k}={v}"))
                .collect();
            format!("{} [{}]", self.name, dims.join(", "))
        }
    }
}

/// A single captured shot: its content digest and, for a live capture, the
/// relative path to its PNG (absent for a digest-only baseline).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Shot {
    /// Hex SHA-256 of the PNG bytes.
    pub(crate) hash: String,
    /// Image path relative to the capture's `captures.json`. `None` in a baseline,
    /// which records digests only (no committed PNGs).
    pub(crate) image: Option<String>,
}

impl Shot {
    /// A shot with a digest and image path.
    pub(crate) fn new(hash: impl Into<String>, image: Option<String>) -> Self {
        Self {
            hash: hash.into(),
            image,
        }
    }
}

/// A capture mapping each [`ShotKey`] to its [`Shot`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Snapshot {
    shots: BTreeMap<ShotKey, Shot>,
}

impl Snapshot {
    /// Create an empty snapshot.
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Record a shot, overwriting any previous entry for the key.
    pub(crate) fn insert(&mut self, key: ShotKey, shot: Shot) {
        self.shots.insert(key, shot);
    }

    /// The shot for `key`, if present.
    pub(crate) fn get(&self, key: &ShotKey) -> Option<&Shot> {
        self.shots.get(key)
    }

    /// The content digest for `key`, if present.
    #[cfg(test)]
    pub(crate) fn digest(&self, key: &ShotKey) -> Option<&str> {
        self.shots.get(key).map(|s| s.hash.as_str())
    }

    /// Keys in `(name, toggles)` order.
    pub(crate) fn keys(&self) -> impl Iterator<Item = &ShotKey> {
        self.shots.keys()
    }

    /// `(key, shot)` pairs in `(name, toggles)` order.
    pub(crate) fn iter(&self) -> impl Iterator<Item = (&ShotKey, &Shot)> {
        self.shots.iter()
    }

    /// Number of shots.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.shots.len()
    }

    /// Whether the snapshot contains no shots.
    pub(crate) fn is_empty(&self) -> bool {
        self.shots.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn label_omits_empty_toggles() {
        assert_eq!(ShotKey::bare("home").label(), "home");
    }

    #[test]
    fn label_lists_toggles_in_key_order() {
        // Insert out of key order to prove the BTreeMap normalizes it.
        let key = ShotKey::with("home", &[("viewport", "mobile"), ("theme", "dark")]);
        assert_eq!(key.label(), "home [theme=dark, viewport=mobile]");
    }

    #[test]
    fn ordering_is_by_name_then_toggles() {
        let mut keys = [
            ShotKey::with("home", &[("theme", "light")]),
            ShotKey::with("about", &[("theme", "dark")]),
            ShotKey::with("home", &[("theme", "dark")]),
        ];
        keys.sort();
        let labels: Vec<String> = keys.iter().map(ShotKey::label).collect();
        assert_eq!(
            labels,
            vec![
                "about [theme=dark]",
                "home [theme=dark]",
                "home [theme=light]",
            ]
        );
    }

    #[test]
    fn snapshot_get_digest_and_len() {
        let mut s = Snapshot::new();
        assert!(s.is_empty());
        let key = ShotKey::with("home", &[("theme", "dark")]);
        s.insert(
            key.clone(),
            Shot::new("ab", Some("home/dark.png".to_owned())),
        );
        assert_eq!(s.len(), 1);
        assert_eq!(s.digest(&key), Some("ab"));
        assert_eq!(
            s.get(&key).and_then(|shot| shot.image.as_deref()),
            Some("home/dark.png")
        );
        assert_eq!(s.digest(&ShotKey::bare("missing")), None);
    }
}
