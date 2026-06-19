//! Declared toggle dimensions: the controls a gallery renders.
//!
//! A *toggle* is a user-defined dimension a screenshot varies over — `theme`,
//! `viewport`, `density`, … — declared once in `screencomp.toml`. The gallery
//! renders one control group per dimension (in declaration order, using each
//! dimension's declared value order) so a single screen is one card you toggle
//! through, rather than one card per variant. Each shot's chosen values live in
//! its [`crate::domain::snapshot::ShotKey`]; this type is the *definition* those
//! values reference.
//!
//! Pure data: it is built from config and passed into the gallery renderer as a
//! plain parameter, keeping `domain` free of any dependency on `config`.

/// One declared toggle dimension: a stable `key`, a display `label`, and the
/// ordered `values` it can take.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ToggleDim {
    /// Stable identifier referenced by each shot's toggle map (e.g. `theme`).
    pub(crate) key: String,
    /// Human-facing label shown above the control group (e.g. `Theme`).
    pub(crate) label: String,
    /// Allowed values in display order; the first is the gallery's default.
    pub(crate) values: Vec<String>,
}

impl ToggleDim {
    /// Whether `value` is one of this dimension's declared values.
    pub(crate) fn allows(&self, value: &str) -> bool {
        self.values.iter().any(|v| v == value)
    }
}

/// Find the declared dimension named `key`, if any.
pub(crate) fn find<'a>(dims: &'a [ToggleDim], key: &str) -> Option<&'a ToggleDim> {
    dims.iter().find(|d| d.key == key)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dims() -> Vec<ToggleDim> {
        vec![
            ToggleDim {
                key: "theme".to_owned(),
                label: "Theme".to_owned(),
                values: vec!["light".to_owned(), "dark".to_owned()],
            },
            ToggleDim {
                key: "viewport".to_owned(),
                label: "Viewport".to_owned(),
                values: vec!["desktop".to_owned(), "mobile".to_owned()],
            },
        ]
    }

    #[test]
    fn allows_checks_declared_values() {
        let d = &dims()[0];
        assert!(d.allows("dark"));
        assert!(!d.allows("sepia"));
    }

    #[test]
    fn find_locates_by_key() {
        let dims = dims();
        assert_eq!(
            find(&dims, "viewport").map(|d| d.label.as_str()),
            Some("Viewport")
        );
        assert!(find(&dims, "density").is_none());
    }
}
