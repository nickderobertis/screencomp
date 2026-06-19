//! Preflight a parsed capture index for the `doctor` command.
//!
//! [`crate::domain::snapshot::Snapshot`] models a capture's content (digests).
//! `doctor` needs a *shape* report on top of that: how many shots, grouped by
//! name; which toggle dimensions and values actually appear; and whether any of
//! those toggles are undeclared in `screencomp.toml` (the classic cause of a
//! gallery control that renders nothing, or a shot the consumer forgot to wire to
//! a dimension). This type is the pure, I/O-free shape the command renders.
//!
//! Whether each shot's referenced PNG exists on disk is an I/O concern the command
//! layers on; it is not decided here.

use super::snapshot::Snapshot;
use super::toggle::{self, ToggleDim};

/// One base name with the number of shots (toggle variants) captured for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NameGroup {
    /// Screenshot base name.
    pub(crate) name: String,
    /// Number of toggle variants captured under this name.
    pub(crate) shots: usize,
}

/// One toggle dimension observed in the capture, with the values it took.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ObservedDim {
    /// Toggle key.
    pub(crate) key: String,
    /// Distinct values seen, in sorted order.
    pub(crate) values: Vec<String>,
}

/// The shape of a capture index: names, observed toggles, and any toggle usage
/// that is not declared in config.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct CapturePreflight {
    /// Base names with their shot counts, in name order.
    pub(crate) names: Vec<NameGroup>,
    /// Observed toggle dimensions, in key order.
    pub(crate) toggles: Vec<ObservedDim>,
    /// Human-readable reasons a shot uses a toggle key/value not declared in
    /// `[[toggle]]`, in sorted order. A gallery cannot render a control for these.
    pub(crate) undeclared: Vec<String>,
}

impl CapturePreflight {
    /// Total shots across all names.
    pub(crate) fn total_shots(&self) -> usize {
        self.names.iter().map(|n| n.shots).sum()
    }

    /// Whether the capture would feed downstream commands nothing, or carries
    /// toggles no declared dimension covers — the two surprises a preflight exists
    /// to surface. (Missing image files are an I/O check the command adds.)
    pub(crate) fn has_problems(&self) -> bool {
        self.total_shots() == 0 || !self.undeclared.is_empty()
    }
}

/// Compute the [`CapturePreflight`] for `snapshot` against the declared `dims`.
pub(crate) fn preflight(snapshot: &Snapshot, dims: &[ToggleDim]) -> CapturePreflight {
    use std::collections::BTreeMap;

    let mut name_counts: BTreeMap<&str, usize> = BTreeMap::new();
    let mut observed: BTreeMap<&str, std::collections::BTreeSet<&str>> = BTreeMap::new();
    let mut undeclared: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();

    for key in snapshot.keys() {
        *name_counts.entry(key.name.as_str()).or_default() += 1;
        for (dim_key, value) in &key.toggles {
            observed
                .entry(dim_key.as_str())
                .or_default()
                .insert(value.as_str());
            match toggle::find(dims, dim_key) {
                None => {
                    undeclared.insert(format!("toggle '{dim_key}' is not declared in [[toggle]]"));
                }
                Some(dim) if !dim.allows(value) => {
                    undeclared.insert(format!(
                        "toggle '{dim_key}' has value '{value}', not in its declared values"
                    ));
                }
                Some(_) => {}
            }
        }
    }

    CapturePreflight {
        names: name_counts
            .into_iter()
            .map(|(name, shots)| NameGroup {
                name: name.to_owned(),
                shots,
            })
            .collect(),
        toggles: observed
            .into_iter()
            .map(|(key, values)| ObservedDim {
                key: key.to_owned(),
                values: values.into_iter().map(str::to_owned).collect(),
            })
            .collect(),
        undeclared: undeclared.into_iter().collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::snapshot::{Shot, ShotKey};

    fn dims() -> Vec<ToggleDim> {
        vec![ToggleDim {
            key: "theme".to_owned(),
            label: "Theme".to_owned(),
            values: vec!["light".to_owned(), "dark".to_owned()],
        }]
    }

    fn snap(keys: &[ShotKey]) -> Snapshot {
        let mut s = Snapshot::new();
        for (i, key) in keys.iter().enumerate() {
            s.insert(key.clone(), Shot::new(format!("{i:064x}"), None));
        }
        s
    }

    #[test]
    fn groups_names_and_observes_toggles() {
        let s = snap(&[
            ShotKey::with("home", &[("theme", "light")]),
            ShotKey::with("home", &[("theme", "dark")]),
            ShotKey::with("about", &[("theme", "light")]),
        ]);
        let p = preflight(&s, &dims());
        assert_eq!(p.total_shots(), 3);
        assert_eq!(
            p.names,
            vec![
                NameGroup {
                    name: "about".to_owned(),
                    shots: 1
                },
                NameGroup {
                    name: "home".to_owned(),
                    shots: 2
                },
            ]
        );
        assert_eq!(
            p.toggles,
            vec![ObservedDim {
                key: "theme".to_owned(),
                values: vec!["dark".to_owned(), "light".to_owned()],
            }]
        );
        assert!(!p.has_problems());
    }

    #[test]
    fn empty_capture_is_a_problem() {
        assert!(preflight(&Snapshot::new(), &dims()).has_problems());
    }

    #[test]
    fn undeclared_key_and_value_are_problems() {
        let s = snap(&[
            ShotKey::with("home", &[("density", "2x")]), // unknown key
            ShotKey::with("home", &[("theme", "sepia")]), // unknown value
        ]);
        let p = preflight(&s, &dims());
        assert!(p.has_problems());
        assert_eq!(p.undeclared.len(), 2, "{:?}", p.undeclared);
        assert!(p.undeclared.iter().any(|u| u.contains("density")));
        assert!(p.undeclared.iter().any(|u| u.contains("sepia")));
    }
}
