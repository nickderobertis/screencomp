//! Compare two snapshots and classify each shot.

use std::collections::BTreeSet;

use super::snapshot::{ShotKey, Snapshot};

/// Per-shot comparison result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Status {
    /// Present in current, absent in baseline.
    Added,
    /// Present in both, differing bytes.
    Changed,
    /// Present in baseline, absent in current.
    Removed,
    /// Present in both, identical bytes.
    Unchanged,
}

impl Status {
    /// Lowercase token used in human output and JSON (`added`, `changed`, ...).
    pub(crate) fn label_lower(self) -> &'static str {
        match self {
            Status::Added => "added",
            Status::Changed => "changed",
            Status::Removed => "removed",
            Status::Unchanged => "unchanged",
        }
    }
}

/// One classified shot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Entry {
    /// Identity (name + toggles) of the shot.
    pub(crate) key: ShotKey,
    /// Comparison result.
    pub(crate) status: Status,
    /// Baseline image path, when the baseline carried one (absent in manifest mode).
    pub(crate) baseline_image: Option<String>,
    /// Current image path, when the current capture carried one.
    pub(crate) current_image: Option<String>,
}

/// Aggregate counts by status.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
pub(crate) struct Counts {
    /// Number of added shots.
    pub(crate) added: usize,
    /// Number of changed shots.
    pub(crate) changed: usize,
    /// Number of removed shots.
    pub(crate) removed: usize,
    /// Number of unchanged shots.
    pub(crate) unchanged: usize,
}

/// Full classification of a current capture against a baseline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Classification {
    /// Entries in `(name, toggles)` order.
    pub(crate) entries: Vec<Entry>,
    /// Aggregate counts.
    pub(crate) counts: Counts,
}

impl Classification {
    /// Whether any shot was added, changed, or removed.
    pub(crate) fn has_changes(&self) -> bool {
        self.counts.added + self.counts.changed + self.counts.removed > 0
    }
}

/// Classify `current` against `baseline` by comparing content digests.
pub(crate) fn classify(baseline: &Snapshot, current: &Snapshot) -> Classification {
    let keys: BTreeSet<&ShotKey> = baseline.keys().chain(current.keys()).collect();

    let mut entries = Vec::with_capacity(keys.len());
    let mut counts = Counts::default();

    for key in keys {
        let base = baseline.get(key);
        let cur = current.get(key);
        let status = match (base, cur) {
            (None, Some(_)) => Status::Added,
            (Some(_), None) => Status::Removed,
            (Some(a), Some(b)) if a.hash == b.hash => Status::Unchanged,
            (Some(_), Some(_)) => Status::Changed,
            (None, None) => unreachable!("key originates from one of the snapshots"),
        };

        match status {
            Status::Added => counts.added += 1,
            Status::Changed => counts.changed += 1,
            Status::Removed => counts.removed += 1,
            Status::Unchanged => counts.unchanged += 1,
        }

        entries.push(Entry {
            key: key.clone(),
            status,
            baseline_image: base.and_then(|s| s.image.clone()),
            current_image: cur.and_then(|s| s.image.clone()),
        });
    }

    Classification { entries, counts }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::snapshot::Shot;

    fn snap(items: &[(ShotKey, &str)]) -> Snapshot {
        let mut s = Snapshot::new();
        for (key, digest) in items {
            s.insert(
                key.clone(),
                Shot::new(*digest, Some(format!("{}.png", key.name))),
            );
        }
        s
    }

    #[test]
    fn detects_every_status() {
        let baseline = snap(&[
            (ShotKey::with("home", &[("theme", "light")]), "aa"),
            (ShotKey::with("about", &[("theme", "light")]), "bb"),
            (ShotKey::with("home", &[("theme", "dark")]), "cc"),
        ]);
        let current = snap(&[
            (ShotKey::with("home", &[("theme", "light")]), "aa"), // unchanged
            (ShotKey::with("about", &[("theme", "light")]), "zz"), // changed
            (ShotKey::with("pricing", &[("theme", "light")]), "dd"), // added
                                                                  // home[theme=dark] removed
        ]);

        let c = classify(&baseline, &current);
        assert_eq!(c.counts.added, 1);
        assert_eq!(c.counts.changed, 1);
        assert_eq!(c.counts.removed, 1);
        assert_eq!(c.counts.unchanged, 1);
        assert!(c.has_changes());

        // Deterministic order: (name, toggles).
        let order: Vec<(String, Status)> = c
            .entries
            .iter()
            .map(|e| (e.key.label(), e.status))
            .collect();
        assert_eq!(
            order,
            vec![
                ("about [theme=light]".to_owned(), Status::Changed),
                ("home [theme=dark]".to_owned(), Status::Removed),
                ("home [theme=light]".to_owned(), Status::Unchanged),
                ("pricing [theme=light]".to_owned(), Status::Added),
            ]
        );
    }

    #[test]
    fn carries_both_image_sides() {
        let baseline = snap(&[(ShotKey::bare("home"), "aa")]);
        let current = snap(&[(ShotKey::bare("home"), "bb")]);
        let c = classify(&baseline, &current);
        let e = &c.entries[0];
        assert_eq!(e.status, Status::Changed);
        assert_eq!(e.baseline_image.as_deref(), Some("home.png"));
        assert_eq!(e.current_image.as_deref(), Some("home.png"));
    }

    #[test]
    fn identical_snapshots_have_no_changes() {
        let s = snap(&[(ShotKey::bare("home"), "aa")]);
        let c = classify(&s, &s);
        assert!(!c.has_changes());
        assert_eq!(c.counts.unchanged, 1);
    }
}
