//! Render the sticky pull-request comment body.
//!
//! The output is deterministic Markdown that begins with an HTML marker comment
//! so a publishing step can find and upsert the same comment across pushes.

use super::classify::{Classification, Entry, Status};

/// Width, in pixels, of inline preview images embedded in the comment.
const EMBED_WIDTH: u32 = 380;

/// Render the comment Markdown.
///
/// `marker` is embedded as `<!-- marker -->` for sticky upserts. `title` heads
/// the comment. When `show_unchanged` is set, unchanged screenshots are listed
/// too. An optional `gallery_url` is appended as a link and, when present, also
/// serves as the base URL for inline image previews.
///
/// When a `gallery_url` is given and at most `embed_limit` screenshots differ
/// (added + changed + removed), those shots are embedded inline — changed ones
/// before/after, added/removed ones as a single image. Above the limit, or with
/// no base URL, the comment falls back to a plain path listing.
pub(crate) fn render_markdown(
    classification: &Classification,
    title: &str,
    marker: &str,
    show_unchanged: bool,
    gallery_url: Option<&str>,
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
    // Embed inline previews only when the diff is small enough and there is a
    // base URL to resolve image paths against; otherwise list the paths.
    let embed_base = gallery_url.filter(|_| differing > 0 && differing <= embed_limit);

    let mut wrote_section = false;
    match embed_base {
        Some(base) => wrote_section |= push_embedded(&mut md, classification, base),
        None => {
            for (status, heading) in [
                (Status::Added, "Added"),
                (Status::Changed, "Changed"),
                (Status::Removed, "Removed"),
            ] {
                wrote_section |= push_section(&mut md, classification, status, heading);
            }
        }
    }
    if show_unchanged {
        wrote_section |= push_section(&mut md, classification, Status::Unchanged, "Unchanged");
    }
    if !wrote_section {
        md.push_str("_No visual changes._\n");
    }

    if let Some(url) = gallery_url {
        md.push_str(&format!("\n[View full gallery]({url})\n"));
    }

    md
}

/// Append inline image previews for every differing shot. Changed shots render
/// before/after side by side; added and removed shots render a single image.
/// `base` is the gallery URL the `baseline/` and `current/` image trees sit
/// under. Returns whether anything was written.
fn push_embedded(md: &mut String, classification: &Classification, base: &str) -> bool {
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
            md.push_str(&format!("**{label}**\n\n"));
            md.push_str("| Before | After |\n| --- | --- |\n");
            md.push_str(&format!(
                "| {} | {} |\n\n",
                img(&image_url(base, "baseline", e), &label),
                img(&image_url(base, "current", e), &label),
            ));
        }
    }

    any |= push_embedded_single(md, classification, Status::Added, "Added", "current", base);
    any |= push_embedded_single(
        md,
        classification,
        Status::Removed,
        "Removed",
        "baseline",
        base,
    );
    any
}

/// Append a section of single inline images for one `status`, sourcing images
/// from `dir` (`current` or `baseline`). Returns whether anything was written.
fn push_embedded_single(
    md: &mut String,
    classification: &Classification,
    status: Status,
    heading: &str,
    dir: &str,
    base: &str,
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
        md.push_str(&format!(
            "**{label}**\n\n{}\n\n",
            img(&image_url(base, dir, e), &label)
        ));
    }
    true
}

/// Public URL of an `entry` image under `dir` (`baseline`/`current`), joined onto
/// `base` with exactly one separating slash.
fn image_url(base: &str, dir: &str, entry: &Entry) -> String {
    format!(
        "{}/{}/{}/{}.png",
        base.trim_end_matches('/'),
        dir,
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

    #[test]
    fn includes_marker_title_and_table() {
        // No base URL: falls back to a path listing regardless of the limit.
        let md = render_markdown(
            &classification(),
            "Visual changes",
            "screencomp",
            false,
            None,
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
        let md = render_markdown(&empty, "Visual changes", "screencomp", false, None, 10);
        assert!(md.contains("_No visual changes._"));
    }

    #[test]
    fn appends_gallery_link() {
        // embed_limit 0 keeps the path-listing fallback even with a base URL.
        let md = render_markdown(
            &classification(),
            "Visual changes",
            "screencomp",
            false,
            Some("https://example.test/g/"),
            0,
        );
        assert!(md.contains("[View full gallery](https://example.test/g/)"));
        assert!(md.contains("### Changed\n- `desktop/home`"));
        assert!(!md.contains("<img"));
    }

    #[test]
    fn embeds_inline_previews_under_the_limit() {
        let md = render_markdown(
            &classification(),
            "Visual changes",
            "screencomp",
            false,
            Some("https://example.test/pr/7/"),
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
    fn falls_back_to_listing_over_the_limit() {
        // Two differing shots (1 added + 1 changed) exceed a limit of 1.
        let md = render_markdown(
            &classification(),
            "Visual changes",
            "screencomp",
            false,
            Some("https://example.test/pr/7/"),
            1,
        );
        assert!(!md.contains("<img"));
        assert!(md.contains("### Added\n- `desktop/pricing`"));
    }
}
