//! Pure helpers for the `doctor --env` environment preflight.
//!
//! The orchestration (running Git, probing `PATH`, reading files) lives in the
//! command and `io` layers; the string parsing that has nothing to do with I/O
//! lives here so it is deterministic and unit-testable.

/// Marker the scaffolded caller workflow pins the reusable workflow with, e.g.
/// `…/visual-docs-reusable.yml@v0.3.0`. The version follows the `@v`.
const PIN_MARKER: &str = "visual-docs-reusable.yml@v";

/// Extract the screencomp version a scaffolded caller workflow pins, if any.
///
/// `init` writes `uses: …/visual-docs-reusable.yml@v<VERSION>`, tying the
/// downstream half (gate, gallery, comment) to the CLI that scaffolded it. When
/// the installed CLI later drifts from that pin, manifest/classify behavior can
/// diverge between the local guard and CI — so `doctor` compares them. Returns
/// the bare version (no leading `v`), or `None` when no such pin is present.
pub(crate) fn workflow_pin(content: &str) -> Option<String> {
    let start = content.find(PIN_MARKER)? + PIN_MARKER.len();
    let version: String = content[start..]
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    // A trailing `@v` with nothing version-like after it is not a usable pin.
    (!version.is_empty()).then_some(version)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_pinned_version_from_a_uses_line() {
        let content = "    uses: nickderobertis/screencomp/.github/workflows/\
                        visual-docs-reusable.yml@v0.3.0\n    with:\n";
        assert_eq!(workflow_pin(content), Some("0.3.0".to_owned()));
    }

    #[test]
    fn no_pin_returns_none() {
        assert_eq!(workflow_pin("name: Visual docs\non: [push]\n"), None);
        // The marker present but with no version after it is not a usable pin.
        assert_eq!(workflow_pin("visual-docs-reusable.yml@vmain"), None);
    }

    #[test]
    fn stops_at_the_first_non_version_character() {
        // Trailing content after the version must not bleed into it.
        assert_eq!(
            workflow_pin("visual-docs-reusable.yml@v1.2.3 # comment"),
            Some("1.2.3".to_owned())
        );
    }
}
