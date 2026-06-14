//! Render the sticky pull-request comment body.
//!
//! The output is deterministic Markdown that begins with an HTML marker comment
//! so a publishing step can find and upsert the same comment across pushes.

use super::classify::{Classification, Entry, Status};

/// Width, in pixels, of inline preview images embedded in the comment.
const EMBED_WIDTH: u32 = 380;

/// Base URLs for the "Before" and "After" images embedded in the comment.
///
/// Each base, when present, hosts its tree in the plain `<base>/<project>/<name>.png`
/// layout (the same layout `gallery` writes without a `--baseline`). The two are
/// decoupled because the "Before" and "After" images do not always live together:
/// in manifest mode there are no committed baseline PNGs at all, so "Before" must
/// come from a separate canonical gallery — or be omitted entirely.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct ImageBases<'a> {
    /// Where baseline ("Before") images are hosted, if anywhere.
    pub(crate) before: Option<&'a str>,
    /// Where current ("After") images are hosted, if anywhere.
    pub(crate) after: Option<&'a str>,
}

impl ImageBases<'_> {
    /// Whether any inline image can be embedded at all.
    fn any(&self) -> bool {
        self.before.is_some() || self.after.is_some()
    }
}

/// Render the comment Markdown.
///
/// `marker` is embedded as `<!-- marker -->` for sticky upserts. `title` heads
/// the comment. When `show_unchanged` is set, unchanged screenshots are listed
/// too. An optional `gallery_link` is appended as a "View full gallery" link.
///
/// When at least one image base in `bases` is available and at most `embed_limit`
/// screenshots differ (added + changed + removed), those shots are embedded
/// inline — changed ones before/after when both bases are set, added/removed (and
/// one-sided changed) ones as a single image. Above the limit, or with no image
/// base, the comment falls back to a plain path listing.
pub(crate) fn render_markdown(
    classification: &Classification,
    title: &str,
    marker: &str,
    show_unchanged: bool,
    gallery_link: Option<&str>,
    bases: ImageBases<'_>,
    embed_limit: usize,
) -> String {
    let counts = classification.counts;
    let mut md = String::new();

    md.push_str(&format!("<!-- {marker} -->\n"));
    md.push_str(&format!("## {title}\n\n"));
    md.push_str("| Added | Changed | Removed | Unchanged |\n");
    md.push_str("|------:|--------:|--------:|----------:|\n");
    md.push_str(&format!(
        "| {} | {} | {} | {} |\n\n",
        counts.added, counts.changed, counts.removed, counts.unchanged
    ));

    let differing = counts.added + counts.changed + counts.removed;
    // Embed inline previews only when the diff is small enough and there is at
    // least one base URL to resolve image paths against; otherwise list the paths.
    let embed = differing > 0 && differing <= embed_limit && bases.any();

    let mut wrote_section = false;
    if embed {
        wrote_section |= push_embedded(&mut md, classification, bases);
    } else {
        for (status, heading) in [
            (Status::Added, "Added"),
            (Status::Changed, "Changed"),
            (Status::Removed, "Removed"),
        ] {
            wrote_section |= push_section(&mut md, classification, status, heading);
        }
    }
    if show_unchanged {
        wrote_section |= push_section(&mut md, classification, Status::Unchanged, "Unchanged");
    }
    if !wrote_section {
        md.push_str("_No visual changes._\n");
    }

    if let Some(url) = gallery_link {
        md.push_str(&format!("\n[View full gallery]({url})\n"));
    }

    md
}

/// Append inline image previews for every differing shot. Changed shots render
/// before/after side by side when both bases are set, and a single image when
/// only one is; added shots come from the "After" base and removed from "Before".
/// A shot whose required base is absent (e.g. "Before" in manifest mode without a
/// baseline URL) falls back to a path bullet so nothing silently 404s. Returns
/// whether anything was written.
fn push_embedded(md: &mut String, classification: &Classification, bases: ImageBases<'_>) -> bool {
    let changed: Vec<&Entry> = classification
        .entries
        .iter()
        .filter(|e| e.status == Status::Changed)
        .collect();
    let mut any = !changed.is_empty();
    if any {
        md.push_str("### Changed\n");
        for e in changed {
            let label = format!("{}/{}", e.project, e.name);
            let before = bases.before.map(|b| image_url(b, e));
            let after = bases.after.map(|b| image_url(b, e));
            match (before, after) {
                (Some(before), Some(after)) => {
                    md.push_str(&format!("**{label}**\n\n"));
                    md.push_str("| Before | After |\n| --- | --- |\n");
                    md.push_str(&format!(
                        "| {} | {} |\n\n",
                        img(&before, &label),
                        img(&after, &label),
                    ));
                }
                // Only one side is hosted (typically manifest mode: just "After").
                (Some(only), None) | (None, Some(only)) => {
                    md.push_str(&format!("**{label}**\n\n{}\n\n", img(&only, &label)));
                }
                (None, None) => unreachable!("embed runs only when a base is set"),
            }
        }
    }

    any |= push_embedded_single(md, classification, Status::Added, "Added", bases.after);
    any |= push_embedded_single(md, classification, Status::Removed, "Removed", bases.before);
    any
}

/// Append a section of single inline images for one `status`, sourcing images
/// from `base` (the "After" base for added shots, "Before" for removed). When the
/// relevant base is absent the shots are listed as path bullets instead, so a
/// missing tree never produces a broken `<img>`. Returns whether anything was
/// written.
fn push_embedded_single(
    md: &mut String,
    classification: &Classification,
    status: Status,
    heading: &str,
    base: Option<&str>,
) -> bool {
    let items: Vec<&Entry> = classification
        .entries
        .iter()
        .filter(|e| e.status == status)
        .collect();
    if items.is_empty() {
        return false;
    }
    md.push_str(&format!("### {heading}\n"));
    for e in items {
        let label = format!("{}/{}", e.project, e.name);
        match base {
            Some(base) => {
                md.push_str(&format!(
                    "**{label}**\n\n{}\n\n",
                    img(&image_url(base, e), &label)
                ));
            }
            None => md.push_str(&format!("- `{label}`\n")),
        }
    }
    if base.is_none() {
        md.push('\n');
    }
    true
}

/// Public URL of an `entry` image under `base`, in the plain
/// `<base>/<project>/<name>.png` layout, joined with exactly one separating slash.
fn image_url(base: &str, entry: &Entry) -> String {
    format!(
        "{}/{}/{}.png",
        base.trim_end_matches('/'),
        entry.project,
        entry.name
    )
}

/// An `<img>` tag constrained to the preview width.
fn img(src: &str, alt: &str) -> String {
    format!("<img src=\"{src}\" alt=\"{alt}\" width=\"{EMBED_WIDTH}\">")
}

/// Append a `### heading` section listing entries with `status`. Returns whether
/// anything was written.
fn push_section(
    md: &mut String,
    classification: &Classification,
    status: Status,
    heading: &str,
) -> bool {
    let mut any = false;
    for entry in classification.entries.iter().filter(|e| e.status == status) {
        if !any {
            md.push_str(&format!("### {heading}\n"));
            any = true;
        }
        md.push_str(&format!("- `{}/{}`\n", entry.project, entry.name));
    }
    if any {
        md.push('\n');
    }
    any
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::classify::{Counts, Entry};

    fn classification() -> Classification {
        Classification {
            entries: vec![
                Entry {
                    project: "desktop".to_owned(),
                    name: "home".to_owned(),
                    status: Status::Changed,
                },
                Entry {
                    project: "desktop".to_owned(),
                    name: "pricing".to_owned(),
                    status: Status::Added,
                },
            ],
            counts: Counts {
                added: 1,
                changed: 1,
                removed: 0,
                unchanged: 4,
            },
        }
    }

    /// Diff-gallery bases: a single deployment with `baseline/` and `current/`
    /// subtrees, the layout `gallery --baseline` writes.
    fn diff_bases(base: &str) -> (String, String) {
        let base = base.trim_end_matches('/');
        (format!("{base}/baseline"), format!("{base}/current"))
    }

    #[test]
    fn includes_marker_title_and_table() {
        // No image base: falls back to a path listing regardless of the limit.
        let md = render_markdown(
            &classification(),
            "Visual changes",
            "screencomp",
            false,
            None,
            ImageBases::default(),
            10,
        );
        assert!(md.starts_with("<!-- screencomp -->\n"));
        assert!(md.contains("## Visual changes"));
        assert!(md.contains("| 1 | 1 | 0 | 4 |"));
        assert!(md.contains("### Added\n- `desktop/pricing`"));
        assert!(md.contains("### Changed\n- `desktop/home`"));
        assert!(!md.contains("### Removed"));
        assert!(!md.contains("<img"));
    }

    #[test]
    fn no_changes_message() {
        let empty = Classification {
            entries: vec![],
            counts: Counts::default(),
        };
        let md = render_markdown(
            &empty,
            "Visual changes",
            "screencomp",
            false,
            None,
            ImageBases::default(),
            10,
        );
        assert!(md.contains("_No visual changes._"));
    }

    #[test]
    fn appends_gallery_link() {
        // embed_limit 0 keeps the path-listing fallback even with image bases.
        let (before, after) = diff_bases("https://example.test/g/");
        let md = render_markdown(
            &classification(),
            "Visual changes",
            "screencomp",
            false,
            Some("https://example.test/g/"),
            ImageBases {
                before: Some(&before),
                after: Some(&after),
            },
            0,
        );
        assert!(md.contains("[View full gallery](https://example.test/g/)"));
        assert!(md.contains("### Changed\n- `desktop/home`"));
        assert!(!md.contains("<img"));
    }

    #[test]
    fn embeds_inline_previews_under_the_limit() {
        let (before, after) = diff_bases("https://example.test/pr/7/");
        let md = render_markdown(
            &classification(),
            "Visual changes",
            "screencomp",
            false,
            Some("https://example.test/pr/7/"),
            ImageBases {
                before: Some(&before),
                after: Some(&after),
            },
            10,
        );
        // Changed shots render before/after from both trees...
        assert!(md.contains("### Changed"));
        assert!(md.contains("| Before | After |"));
        assert!(md.contains(
            "<img src=\"https://example.test/pr/7/baseline/desktop/home.png\" \
             alt=\"desktop/home\" width=\"380\">"
        ));
        assert!(md.contains(
            "<img src=\"https://example.test/pr/7/current/desktop/home.png\" \
             alt=\"desktop/home\" width=\"380\">"
        ));
        // ...and added shots render a single image from `current`.
        assert!(md.contains("### Added"));
        assert!(md.contains("src=\"https://example.test/pr/7/current/desktop/pricing.png\""));
        // No path-listing bullets in embed mode; gallery link still present.
        assert!(!md.contains("- `desktop/pricing`"));
        assert!(md.contains("[View full gallery](https://example.test/pr/7/)"));
    }

    #[test]
    fn manifest_mode_embeds_only_after_when_baseline_unhosted() {
        // Manifest mode hosts no baseline PNGs, so only the "After" base is set:
        // changed shots show a single (current) image and removed shots fall back
        // to a path bullet rather than a broken baseline `<img>`.
        let classification = Classification {
            entries: vec![
                Entry {
                    project: "desktop".to_owned(),
                    name: "home".to_owned(),
                    status: Status::Changed,
                },
                Entry {
                    project: "mobile".to_owned(),
                    name: "home".to_owned(),
                    status: Status::Removed,
                },
            ],
            counts: Counts {
                added: 0,
                changed: 1,
                removed: 1,
                unchanged: 0,
            },
        };
        let md = render_markdown(
            &classification,
            "Visual changes",
            "screencomp",
            false,
            None,
            ImageBases {
                before: None,
                after: Some("https://example.test/site"),
            },
            10,
        );
        // Changed: a single current image, never a Before/After table.
        assert!(md.contains("### Changed"), "{md}");
        assert!(!md.contains("| Before | After |"), "{md}");
        assert!(
            md.contains("src=\"https://example.test/site/desktop/home.png\""),
            "{md}"
        );
        // Removed: no baseline base, so a bullet instead of a 404 image.
        assert!(md.contains("### Removed\n- `mobile/home`"), "{md}");
    }

    #[test]
    fn manifest_mode_sources_before_from_a_separate_baseline_url() {
        // With an explicit baseline URL (the canonical/main gallery), manifest
        // mode regains a real Before/After diff: Before from the baseline base,
        // After from the current base.
        let md = render_markdown(
            &classification(),
            "Visual changes",
            "screencomp",
            false,
            None,
            ImageBases {
                before: Some("https://example.test/main"),
                after: Some("https://example.test/pr/7"),
            },
            10,
        );
        assert!(md.contains("| Before | After |"), "{md}");
        assert!(
            md.contains("src=\"https://example.test/main/desktop/home.png\""),
            "{md}"
        );
        assert!(
            md.contains("src=\"https://example.test/pr/7/desktop/home.png\""),
            "{md}"
        );
    }

    #[test]
    fn falls_back_to_listing_over_the_limit() {
        // Two differing shots (1 added + 1 changed) exceed a limit of 1.
        let (before, after) = diff_bases("https://example.test/pr/7/");
        let md = render_markdown(
            &classification(),
            "Visual changes",
            "screencomp",
            false,
            Some("https://example.test/pr/7/"),
            ImageBases {
                before: Some(&before),
                after: Some(&after),
            },
            1,
        );
        assert!(!md.contains("<img"));
        assert!(md.contains("### Added\n- `desktop/pricing`"));
    }
}
