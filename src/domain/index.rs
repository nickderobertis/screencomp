//! The `captures.json` index: the on-disk source of truth for a capture.
//!
//! Each entry carries a shot's base name, its toggle values, the hex SHA-256 of
//! its PNG, and (for a live capture) the relative path to that PNG. This single
//! schema serves both roles:
//!
//! - a **live capture** index (`captures.json`), written by the consumer's
//!   capture step, with `image` set so the gallery can find each PNG;
//! - a **baseline** index, written by `screencomp manifest`, with `image` omitted
//!   — a digest-only record committed in place of the PNGs.
//!
//! ```json
//! {
//!   "schema": 1,
//!   "shots": [
//!     { "name": "home", "toggles": { "theme": "dark" },
//!       "hash": "<64 hex>", "image": "home/dark.png" }
//!   ]
//! }
//! ```
//!
//! Parsing is strict: a wrong schema, a malformed digest, an empty name, or a
//! duplicate `(name, toggles)` key is rejected with a human-readable reason so a
//! hand-edited or truncated index fails loudly instead of silently dropping shots.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::snapshot::{Shot, ShotKey, Snapshot};

/// Current `captures.json` schema version. Bumped only on a breaking change to
/// the on-disk shape; a mismatch is a hard parse error.
pub(crate) const SCHEMA: u32 = 1;

/// A parsed (or to-be-rendered) capture index.
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct CaptureIndex {
    /// On-disk schema version; must equal [`SCHEMA`].
    pub(crate) schema: u32,
    /// The shots, in file order (normalized to `(name, toggles)` order on render).
    #[serde(default)]
    pub(crate) shots: Vec<IndexShot>,
}

/// One shot entry in the index.
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct IndexShot {
    /// Screenshot base name.
    pub(crate) name: String,
    /// Toggle values that produced this shot.
    #[serde(default)]
    pub(crate) toggles: BTreeMap<String, String>,
    /// Hex SHA-256 of the PNG bytes.
    pub(crate) hash: String,
    /// Image path relative to this index; omitted in a baseline.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) image: Option<String>,
}

impl CaptureIndex {
    /// Validate and convert into a content-addressed [`Snapshot`].
    ///
    /// Returns a human-readable reason on malformation; the I/O caller adds the
    /// path context. Rejects a wrong schema, a non-64-hex digest, an empty name,
    /// and a duplicate `(name, toggles)` key.
    pub(crate) fn into_snapshot(self) -> Result<Snapshot, String> {
        if self.schema != SCHEMA {
            return Err(format!(
                "unsupported schema {} (this screencomp reads schema {SCHEMA})",
                self.schema
            ));
        }

        let mut snapshot = Snapshot::new();
        for shot in self.shots {
            if shot.name.is_empty() {
                return Err("a shot has an empty name".to_owned());
            }
            if shot.hash.len() != 64 || !shot.hash.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err(format!(
                    "shot '{}' has '{}', which is not a 64-character hex digest",
                    shot.name, shot.hash
                ));
            }
            let key = ShotKey {
                name: shot.name,
                toggles: shot.toggles,
            };
            if snapshot.get(&key).is_some() {
                return Err(format!("duplicate shot '{}'", key.label()));
            }
            snapshot.insert(key, Shot::new(shot.hash, shot.image));
        }
        Ok(snapshot)
    }

    /// Build an index from a `snapshot`, dropping image paths when `with_images`
    /// is false (the baseline case). Shots are emitted in `(name, toggles)` order.
    fn from_snapshot(snapshot: &Snapshot, with_images: bool) -> Self {
        let shots = snapshot
            .iter()
            .map(|(key, shot)| IndexShot {
                name: key.name.clone(),
                toggles: key.toggles.clone(),
                hash: shot.hash.clone(),
                image: with_images.then(|| shot.image.clone()).flatten(),
            })
            .collect();
        Self {
            schema: SCHEMA,
            shots,
        }
    }
}

/// Render `snapshot` as a pretty-printed, digest-only baseline index.
///
/// Image paths are dropped (a baseline commits no PNGs) and shots are emitted in
/// `(name, toggles)` order, so the output is byte-stable and diffs cleanly.
pub(crate) fn render_baseline(snapshot: &Snapshot) -> String {
    let index = CaptureIndex::from_snapshot(snapshot, false);
    // BTreeMap toggles serialize in key order; the shot order is already
    // normalized, so the only nondeterminism would be a trailing newline — add one.
    let mut json = serde_json::to_string_pretty(&index).expect("index serializes");
    json.push('\n');
    json
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(json: &str) -> Result<Snapshot, String> {
        let index: CaptureIndex = serde_json::from_str(json).map_err(|e| e.to_string())?;
        index.into_snapshot()
    }

    #[test]
    fn parses_shots_with_toggles_and_images() {
        let hash = "a".repeat(64);
        let json = format!(
            r#"{{"schema":1,"shots":[
                {{"name":"home","toggles":{{"theme":"dark"}},"hash":"{hash}","image":"home/dark.png"}}
            ]}}"#
        );
        let snap = parse(&json).expect("parses");
        assert_eq!(snap.len(), 1);
        let key = ShotKey::with("home", &[("theme", "dark")]);
        assert_eq!(snap.digest(&key), Some(hash.as_str()));
        assert_eq!(
            snap.get(&key).and_then(|s| s.image.as_deref()),
            Some("home/dark.png")
        );
    }

    #[test]
    fn rejects_wrong_schema() {
        let hash = "b".repeat(64);
        let err = parse(&format!(
            r#"{{"schema":2,"shots":[{{"name":"a","hash":"{hash}"}}]}}"#
        ))
        .unwrap_err();
        assert!(err.contains("schema"), "{err}");
    }

    #[test]
    fn rejects_bad_digest_and_empty_name() {
        assert!(
            parse(r#"{"schema":1,"shots":[{"name":"a","hash":"nothex"}]}"#)
                .unwrap_err()
                .contains("hex digest")
        );
        let hash = "c".repeat(64);
        assert!(
            parse(&format!(
                r#"{{"schema":1,"shots":[{{"name":"","hash":"{hash}"}}]}}"#
            ))
            .unwrap_err()
            .contains("empty name")
        );
    }

    #[test]
    fn rejects_duplicate_keys() {
        let hash = "d".repeat(64);
        let json = format!(
            r#"{{"schema":1,"shots":[
                {{"name":"home","toggles":{{"theme":"dark"}},"hash":"{hash}"}},
                {{"name":"home","toggles":{{"theme":"dark"}},"hash":"{hash}"}}
            ]}}"#
        );
        assert!(parse(&json).unwrap_err().contains("duplicate"));
    }

    #[test]
    fn baseline_round_trips_without_images_and_is_sorted() {
        let mut snap = Snapshot::new();
        let h1 = "1".repeat(64);
        let h2 = "2".repeat(64);
        // Insert out of order; render must sort and drop images.
        snap.insert(
            ShotKey::with("home", &[("theme", "light")]),
            Shot::new(h2.clone(), Some("home/light.png".to_owned())),
        );
        snap.insert(
            ShotKey::with("about", &[("theme", "dark")]),
            Shot::new(h1.clone(), Some("about/dark.png".to_owned())),
        );

        let rendered = render_baseline(&snap);
        assert!(rendered.ends_with('\n'));
        assert!(
            !rendered.contains("image"),
            "baseline drops images: {rendered}"
        );
        // about sorts before home.
        assert!(rendered.find("about").unwrap() < rendered.find("home").unwrap());

        // Re-parsing yields the same digests (images now absent).
        let reparsed = parse(&rendered).expect("baseline re-parses");
        assert_eq!(
            reparsed.digest(&ShotKey::with("about", &[("theme", "dark")])),
            Some(h1.as_str())
        );
        assert_eq!(
            reparsed
                .get(&ShotKey::with("home", &[("theme", "light")]))
                .and_then(|s| s.image.as_deref()),
            None
        );
    }
}
