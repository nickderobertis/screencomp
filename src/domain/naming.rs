//! Derive a shot's identity from the relative path of its PNG.
//!
//! The `captures.json` index keys every shot by a `name` plus a toggle map, but a
//! capture step only ever produces *files*. `index` therefore has to read that
//! identity back out of the path each screenshot was written to, which this module
//! defines:
//!
//! - the name is the relative path with its `.png` extension dropped, so
//!   `home.png` is `home` and `checkout/step-2.png` is `checkout/step-2`;
//! - with [`Naming::toggles_from_path`], any path *segment* shaped like
//!   `key=value` is consumed as a toggle instead of becoming part of the name, so
//!   `theme=dark/home.png` and `home/theme=dark.png` both describe
//!   `home [theme=dark]`;
//! - [`Naming::fixed`] toggles apply to every shot, for a capture pass that
//!   varies a dimension without encoding it in the tree.
//!
//! Pure string work: it takes a `/`-separated relative path (the caller
//! normalizes) and returns a key or a human-readable reason.

use super::snapshot::{ShotKey, Toggles};

/// Extension every indexed screenshot carries, matched case-insensitively so a
/// capture step that writes `.PNG` is still indexed.
const PNG_EXT: &str = "png";

/// Whether `file_name` names a PNG. What makes a file a shot rather than
/// something the capture left lying around (the index itself, a gallery, a note).
pub(crate) fn is_png(file_name: &str) -> bool {
    matches!(file_name.rsplit_once('.'), Some((_, ext)) if ext.eq_ignore_ascii_case(PNG_EXT))
}

/// How [`shot_key`] turns a path into a shot identity.
#[derive(Debug, Clone, Default)]
pub(crate) struct Naming {
    /// Toggles applied to every shot, whatever its path.
    pub(crate) fixed: Toggles,
    /// Whether `key=value` path segments are consumed as toggles.
    pub(crate) toggles_from_path: bool,
}

/// Derive the [`ShotKey`] for the screenshot at the relative path `image`.
///
/// `image` is `/`-separated and includes the `.png` extension. Returns a
/// human-readable reason when the path cannot name a shot: nothing left for the
/// name once toggle segments are consumed, or a path toggle that contradicts a
/// [`Naming::fixed`] one (silently letting either win would make the index
/// disagree with the tree it was built from).
pub(crate) fn shot_key(image: &str, naming: &Naming) -> Result<ShotKey, String> {
    let stem = strip_png_extension(image);
    let mut toggles = naming.fixed.clone();
    let mut segments = Vec::new();

    for segment in stem.split('/') {
        let toggle = if naming.toggles_from_path {
            split_toggle(segment)
        } else {
            None
        };
        match toggle {
            Some((key, value)) => {
                if let Some(existing) = toggles.insert(key.to_owned(), value.to_owned())
                    && existing != value
                {
                    return Err(format!(
                        "toggle '{key}' is set twice with different values \
                         ('{existing}' and '{value}')"
                    ));
                }
            }
            None => segments.push(segment),
        }
    }

    let name = segments.join("/");
    if name.is_empty() {
        return Err("every path segment is a toggle, leaving no shot name".to_owned());
    }
    Ok(ShotKey { name, toggles })
}

/// Drop a trailing `.png` (any case) from `image`.
fn strip_png_extension(image: &str) -> &str {
    match image.rsplit_once('.') {
        Some((stem, ext)) if ext.eq_ignore_ascii_case(PNG_EXT) => stem,
        _ => image,
    }
}

/// Split a `key=value` path segment, if it is one.
///
/// The key is held to the same `[A-Za-z0-9_-]` shape as a `--toggle` key and both
/// halves must be non-empty, so an ordinary filename containing `=` is left alone
/// to become part of the name rather than silently turning into a toggle.
fn split_toggle(segment: &str) -> Option<(&str, &str)> {
    let (key, value) = segment.split_once('=')?;
    let valid_key = !key.is_empty()
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    (valid_key && !value.is_empty()).then_some((key, value))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat() -> Naming {
        Naming::default()
    }

    fn from_path() -> Naming {
        Naming {
            toggles_from_path: true,
            ..Naming::default()
        }
    }

    fn fixed(pairs: &[(&str, &str)]) -> Toggles {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    }

    #[test]
    fn is_png_matches_the_extension_case_insensitively() {
        assert!(is_png("home.png") && is_png("home.PNG"));
        assert!(!is_png("captures.json") && !is_png("index.html") && !is_png("notes"));
    }

    #[test]
    fn flat_name_is_the_path_without_the_extension() {
        assert_eq!(
            shot_key("home.png", &flat()).unwrap(),
            ShotKey::bare("home")
        );
        assert_eq!(
            shot_key("checkout/step-2.png", &flat()).unwrap(),
            ShotKey::bare("checkout/step-2")
        );
    }

    #[test]
    fn extension_match_is_case_insensitive_and_only_trailing() {
        assert_eq!(
            shot_key("home.PNG", &flat()).unwrap(),
            ShotKey::bare("home")
        );
        // A dot inside the name is preserved; only the extension is stripped.
        assert_eq!(
            shot_key("home.v2.png", &flat()).unwrap(),
            ShotKey::bare("home.v2")
        );
    }

    #[test]
    fn flat_naming_ignores_toggle_shaped_segments() {
        assert_eq!(
            shot_key("theme=dark/home.png", &flat()).unwrap(),
            ShotKey::bare("theme=dark/home")
        );
    }

    #[test]
    fn path_toggles_come_from_directories_or_filenames() {
        let expected = ShotKey::with("home", &[("theme", "dark")]);
        assert_eq!(
            shot_key("theme=dark/home.png", &from_path()).unwrap(),
            expected
        );
        assert_eq!(
            shot_key("home/theme=dark.png", &from_path()).unwrap(),
            expected
        );
    }

    #[test]
    fn path_toggles_accumulate_across_segments() {
        assert_eq!(
            shot_key("theme=dark/home/viewport=mobile.png", &from_path()).unwrap(),
            ShotKey::with("home", &[("theme", "dark"), ("viewport", "mobile")])
        );
    }

    #[test]
    fn fixed_toggles_apply_to_every_shot() {
        let naming = Naming {
            fixed: fixed(&[("project", "shop")]),
            toggles_from_path: true,
        };
        assert_eq!(
            shot_key("theme=dark/home.png", &naming).unwrap(),
            ShotKey::with("home", &[("project", "shop"), ("theme", "dark")])
        );
    }

    #[test]
    fn a_path_toggle_may_restate_but_not_contradict_a_fixed_one() {
        let naming = Naming {
            fixed: fixed(&[("theme", "dark")]),
            toggles_from_path: true,
        };
        assert_eq!(
            shot_key("theme=dark/home.png", &naming).unwrap(),
            ShotKey::with("home", &[("theme", "dark")])
        );
        let err = shot_key("theme=light/home.png", &naming).unwrap_err();
        assert!(err.contains("'theme'"), "{err}");
        assert!(err.contains("'dark'") && err.contains("'light'"), "{err}");
    }

    #[test]
    fn two_path_toggles_for_one_key_must_agree() {
        assert!(shot_key("theme=dark/theme=dark/home.png", &from_path()).is_ok());
        let err = shot_key("theme=dark/theme=light/home.png", &from_path()).unwrap_err();
        assert!(err.contains("set twice"), "{err}");
    }

    #[test]
    fn a_segment_that_is_not_key_equals_value_stays_in_the_name() {
        // Empty halves and non-conforming keys are filenames, not toggles.
        assert_eq!(
            shot_key("=dark/home.png", &from_path()).unwrap(),
            ShotKey::bare("=dark/home")
        );
        assert_eq!(
            shot_key("theme=.png", &from_path()).unwrap(),
            ShotKey::bare("theme=")
        );
        assert_eq!(
            shot_key("a b=dark.png", &from_path()).unwrap(),
            ShotKey::bare("a b=dark")
        );
    }

    #[test]
    fn a_path_of_only_toggles_has_no_name() {
        let err = shot_key("theme=dark/viewport=mobile.png", &from_path()).unwrap_err();
        assert!(err.contains("no shot name"), "{err}");
    }
}
