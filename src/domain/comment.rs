//! Render the sticky pull-request comment body.
//!
//! The output is deterministic Markdown that begins with an HTML marker comment
//! so a publishing step can find and upsert the same comment across pushes.

use super::classify::{Classification, Entry, Status};

/// Width, in pixels, of inline preview images embedded in the comment.
const EMBED_WIDTH: u32 = 380;

/// Base URLs for the "Before" and "After" images embedded in the comment.
///
/// Each base, when present, hosts its tree so that a shot's relative `image` path
/// resolves under it as `<base>/<image>` (the same layout `gallery` writes). The
/// two are decoupled because the "Before" and "After" images do not always live
/// together: in manifest mode there are no committed baseline PNGs at all, so
/// "Before" must come from a separate canonical gallery — or be omitted entirely.
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
/// the comment. When `show_unchanged` is set, unchanged shots are listed too. An
/// optional `gallery_link` is appended as a "View full gallery" link.
///
/// When at least one image base in `bases` is available and at most `embed_limit`
/// shots differ (added + changed + removed), those shots are embedded inline —
/// changed ones before/after when both sides resolve, added/removed (and one-sided
/// changed) ones as a single image. Above the limit, or with no resolvable image,
/// the comment falls back to a plain label listing.
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
    // least one base URL to resolve image paths against; otherwise list the labels.
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

/// Public URL of an image under `base`, joined with exactly one separating slash.
fn image_url(base: &str, image: &str) -> String {
    format!("{}/{}", base.trim_end_matches('/'), image)
}

/// Resolve a shot's hosted URL for one side: requires both a base and the shot's
/// image path on that side.
fn side_url(base: Option<&str>, image: Option<&str>) -> Option<String> {
    Some(image_url(base?, image?))
}

/// Append inline image previews for every differing shot. Changed shots render
/// before/after side by side when both sides resolve, and a single image when only
/// one does; added shots come from the "After" base and removed from "Before". The
/// "Before" image path falls back to the current side's path so a `--baseline-url`
/// canonical gallery still yields a real diff in manifest mode (where the baseline
/// carries no image). A shot whose required side does not resolve falls back to a
/// label bullet so nothing silently 404s. Returns whether anything was written.
fn push_embedded(md: &mut String, classification: &Classification, bases: ImageBases<'_>) -> bool {
    let changed: Vec<&Entry> = classification
        .entries
        .iter()
        .filter(|e| e.status == Status::Changed)
        .collect();
    let mut any = false;
    let mut bullets: Vec<String> = Vec::new();
    if !changed.is_empty() {
        let mut section = String::new();
        for e in &changed {
            let label = e.key.label();
            // The "Before" image path falls back to the current side's path: a
            // baseline written by `manifest` carries no image, but a separate
            // canonical gallery (`--baseline-url`) hosts the same shot at the same
            // relative path, so this still resolves a real before/after in manifest
            // mode (the documented use of `--baseline-url`).
            let before_image = e.baseline_image.as_deref().or(e.current_image.as_deref());
            let before = side_url(bases.before, before_image);
            let after = side_url(bases.after, e.current_image.as_deref());
            match (before, after) {
                (Some(before), Some(after)) => {
                    section.push_str(&format!("**{label}**\n\n"));
                    section.push_str("| Before | After |\n| --- | --- |\n");
                    section.push_str(&format!(
                        "| {} | {} |\n\n",
                        img(&before, &label),
                        img(&after, &label),
                    ));
                }
                (Some(only), None) | (None, Some(only)) => {
                    section.push_str(&format!("**{label}**\n\n{}\n\n", img(&only, &label)));
                }
                (None, None) => bullets.push(format!("- `{label}`\n")),
            }
        }
        if !section.is_empty() {
            md.push_str("### Changed\n");
            md.push_str(&section);
            any = true;
        }
        if !bullets.is_empty() {
            if !any {
                md.push_str("### Changed\n");
            }
            for b in &bullets {
                md.push_str(b);
            }
            md.push('\n');
            any = true;
        }
    }

    any |= push_embedded_single(
        md,
        classification,
        Status::Added,
        "Added",
        bases.after,
        true,
    );
    any |= push_embedded_single(
        md,
        classification,
        Status::Removed,
        "Removed",
        bases.before,
        false,
    );
    any
}

/// Append a section of single inline images for one `status`, sourcing each shot's
/// image from `base` (the "After" base for added shots, "Before" for removed). The
/// `current` flag selects which side's image path to use. When a shot does not
/// resolve (no base, or no image on that side) it is listed as a label bullet
/// instead. Returns whether anything was written.
fn push_embedded_single(
    md: &mut String,
    classification: &Classification,
    status: Status,
    heading: &str,
    base: Option<&str>,
    current: bool,
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
        let label = e.key.label();
        let image = if current {
            e.current_image.as_deref()
        } else {
            e.baseline_image.as_deref()
        };
        match side_url(base, image) {
            Some(url) => md.push_str(&format!("**{label}**\n\n{}\n\n", img(&url, &label))),
            None => md.push_str(&format!("- `{label}`\n")),
        }
    }
    md.push('\n');
    true
}

/// An `<img>` tag constrained to the preview width.
fn img(src: &str, alt: &str) -> String {
    format!("<img src=\"{src}\" alt=\"{alt}\" width=\"{EMBED_WIDTH}\">")
}

/// Append a `### heading` section listing entries with `status` by label. Returns
/// whether anything was written.
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
        md.push_str(&format!("- `{}`\n", entry.key.label()));
    }
    if any {
        md.push('\n');
    }
    any
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::classify::Counts;
    use crate::domain::snapshot::ShotKey;

    fn entry(name: &str, status: Status) -> Entry {
        Entry {
            key: ShotKey::with(name, &[("theme", "dark")]),
            status,
            baseline_image: Some(format!("{name}-dark.png")),
            current_image: Some(format!("{name}-dark.png")),
        }
    }

    fn classification() -> Classification {
        Classification {
            entries: vec![
                entry("home", Status::Changed),
                entry("pricing", Status::Added),
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
        assert!(md.contains("### Added\n- `pricing [theme=dark]`"));
        assert!(md.contains("### Changed\n- `home [theme=dark]`"));
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
        assert!(md.contains("### Changed"));
        assert!(md.contains("| Before | After |"));
        assert!(md.contains(
            "<img src=\"https://example.test/pr/7/baseline/home-dark.png\" \
             alt=\"home [theme=dark]\" width=\"380\">"
        ));
        assert!(md.contains(
            "<img src=\"https://example.test/pr/7/current/home-dark.png\" \
             alt=\"home [theme=dark]\" width=\"380\">"
        ));
        assert!(md.contains("### Added"));
        assert!(md.contains("src=\"https://example.test/pr/7/current/pricing-dark.png\""));
        assert!(!md.contains("- `pricing [theme=dark]`"));
        assert!(md.contains("[View full gallery](https://example.test/pr/7/)"));
    }

    #[test]
    fn manifest_mode_embeds_only_after_when_baseline_unhosted() {
        let classification = Classification {
            entries: vec![
                Entry {
                    baseline_image: None,
                    ..entry("home", Status::Changed)
                },
                Entry {
                    current_image: None,
                    ..entry("legacy", Status::Removed)
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
        assert!(md.contains("### Changed"), "{md}");
        assert!(!md.contains("| Before | After |"), "{md}");
        assert!(
            md.contains("src=\"https://example.test/site/home-dark.png\""),
            "{md}"
        );
        // Removed has no baseline base, so a bullet instead of a 404 image.
        assert!(md.contains("### Removed\n- `legacy [theme=dark]`"), "{md}");
    }

    #[test]
    fn manifest_mode_sources_before_from_a_separate_baseline_url() {
        // With an explicit baseline URL (a canonical/main gallery), manifest mode
        // regains a real Before/After diff even though the baseline carries no
        // image: the shot's current image path resolves under both bases.
        let classification = Classification {
            entries: vec![Entry {
                baseline_image: None, // manifest baseline: no committed image
                ..entry("home", Status::Changed)
            }],
            counts: Counts {
                changed: 1,
                ..Counts::default()
            },
        };
        let md = render_markdown(
            &classification,
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
            md.contains("src=\"https://example.test/main/home-dark.png\""),
            "{md}"
        );
        assert!(
            md.contains("src=\"https://example.test/pr/7/home-dark.png\""),
            "{md}"
        );
    }

    #[test]
    fn falls_back_to_listing_over_the_limit() {
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
        assert!(md.contains("### Added\n- `pricing [theme=dark]`"));
    }
}
