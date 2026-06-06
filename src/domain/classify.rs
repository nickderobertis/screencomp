//! Compare two snapshots and classify each screenshot.

use std::collections::BTreeSet;

use serde::Serialize;

use super::snapshot::{ShotKey, Snapshot};

/// Per-screenshot comparison result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
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

/// One classified screenshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct Entry {
    /// Project/variant directory name.
    pub(crate) project: String,
    /// Screenshot base name.
    pub(crate) name: String,
    /// Comparison result.
    pub(crate) status: Status,
}

/// Aggregate counts by status.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub(crate) struct Counts {
    /// Number of added screenshots.
    pub(crate) added: usize,
    /// Number of changed screenshots.
    pub(crate) changed: usize,
    /// Number of removed screenshots.
    pub(crate) removed: usize,
    /// Number of unchanged screenshots.
    pub(crate) unchanged: usize,
}

/// Full classification of a current capture against a baseline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Classification {
    /// Entries in `(project, name)` order.
    pub(crate) entries: Vec<Entry>,
    /// Aggregate counts.
    pub(crate) counts: Counts,
}

impl Classification {
    /// Whether any screenshot was added, changed, or removed.
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
        let status = match (baseline.get(key), current.get(key)) {
            (None, Some(_)) => Status::Added,
            (Some(_), None) => Status::Removed,
            (Some(a), Some(b)) if a == b => Status::Unchanged,
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
            project: key.project.clone(),
            name: key.name.clone(),
            status,
        });
    }

    Classification { entries, counts }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(items: &[(&str, &str, &str)]) -> Snapshot {
        let mut s = Snapshot::new();
        for (project, name, digest) in items {
            s.insert(
                ShotKey {
                    project: (*project).to_owned(),
                    name: (*name).to_owned(),
                },
                (*digest).to_owned(),
            );
        }
        s
    }

    #[test]
    fn detects_every_status() {
        let baseline = snap(&[
            ("desktop", "home", "aa"),
            ("desktop", "about", "bb"),
            ("mobile", "home", "cc"),
        ]);
        let current = snap(&[
            ("desktop", "home", "aa"),  // unchanged
            ("desktop", "about", "zz"), // changed
            ("desktop", "pricing", "dd"), // added
                                        // mobile/home removed
        ]);

        let c = classify(&baseline, &current);
        assert_eq!(c.counts.added, 1);
        assert_eq!(c.counts.changed, 1);
        assert_eq!(c.counts.removed, 1);
        assert_eq!(c.counts.unchanged, 1);
        assert!(c.has_changes());

        // Deterministic order: (project, name).
        let order: Vec<(&str, Status)> = c
            .entries
            .iter()
            .map(|e| (e.name.as_str(), e.status))
            .collect();
        assert_eq!(
            order,
            vec![
                ("about", Status::Changed),
                ("home", Status::Unchanged),
                ("pricing", Status::Added),
                ("home", Status::Removed),
            ]
        );
    }

    #[test]
    fn identical_snapshots_have_no_changes() {
        let s = snap(&[("desktop", "home", "aa")]);
        let c = classify(&s, &s);
        assert!(!c.has_changes());
        assert_eq!(c.counts.unchanged, 1);
    }
}
