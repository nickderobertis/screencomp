//! In-memory description of a screenshot tree's *shape*, for preflight checks.
//!
//! [`crate::domain::snapshot::Snapshot`] models a tree's content (digests) and
//! deliberately discards everything else — empty project directories, files that
//! sit in the wrong place. The `doctor` preflight needs exactly that discarded
//! structure to catch layout mistakes (a stray `home.png` at the root, an empty
//! capture) *before* a classify run reports a misleading empty diff. This type
//! is the pure, I/O-free shape the `io` layer fills and the command renders.

/// One project directory found directly under the scanned root, with the number
/// of `<name>.png` files it contains.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProjectScan {
    /// Project/variant directory name.
    pub(crate) name: String,
    /// Count of `.png` files directly inside it (matching `discover`'s rule).
    pub(crate) shots: usize,
}

/// The structural shape of a screenshot tree as seen one level deep, mirroring
/// the `<root>/<project>/<name>.png` traversal that `discover` performs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct LayoutScan {
    /// Project directories under the root, in name order.
    pub(crate) projects: Vec<ProjectScan>,
    /// `.png` files sitting *directly* under the root, in name order. These are
    /// misplaced: the layout expects `<project>/<name>.png`, so a capture that
    /// writes them is silently invisible to every command.
    pub(crate) loose_pngs: Vec<String>,
}

impl LayoutScan {
    /// Total screenshots that `classify`/`manifest` would actually see.
    pub(crate) fn total_shots(&self) -> usize {
        self.projects.iter().map(|p| p.shots).sum()
    }

    /// Whether the layout would feed those commands nothing, or something they
    /// silently ignore — the two surprises a preflight exists to surface. A
    /// capture with no discoverable shots, or with `.png` files stranded at the
    /// root, is treated as a problem.
    pub(crate) fn has_problems(&self) -> bool {
        self.total_shots() == 0 || !self.loose_pngs.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scan(projects: &[(&str, usize)], loose: &[&str]) -> LayoutScan {
        LayoutScan {
            projects: projects
                .iter()
                .map(|(name, shots)| ProjectScan {
                    name: (*name).to_owned(),
                    shots: *shots,
                })
                .collect(),
            loose_pngs: loose.iter().map(|s| (*s).to_owned()).collect(),
        }
    }

    #[test]
    fn total_shots_sums_projects() {
        assert_eq!(scan(&[("desktop", 3), ("mobile", 1)], &[]).total_shots(), 4);
        assert_eq!(scan(&[], &[]).total_shots(), 0);
    }

    #[test]
    fn a_well_formed_nonempty_tree_has_no_problems() {
        assert!(!scan(&[("desktop", 2)], &[]).has_problems());
    }

    #[test]
    fn empty_tree_is_a_problem() {
        assert!(scan(&[], &[]).has_problems());
        // A project directory with no PNGs contributes zero shots.
        assert!(scan(&[("desktop", 0)], &[]).has_problems());
    }

    #[test]
    fn loose_pngs_are_a_problem_even_with_real_shots() {
        assert!(scan(&[("desktop", 2)], &["home.png"]).has_problems());
    }
}
