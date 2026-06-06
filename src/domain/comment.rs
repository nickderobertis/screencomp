//! Render the sticky pull-request comment body.
//!
//! The output is deterministic Markdown that begins with an HTML marker comment
//! so a publishing step can find and upsert the same comment across pushes.

use super::classify::{Classification, Status};

/// Render the comment Markdown.
///
/// `marker` is embedded as `<!-- marker -->` for sticky upserts. `title` heads
/// the comment. When `show_unchanged` is set, unchanged screenshots are listed
/// too. An optional `gallery_url` is appended as a link.
pub(crate) fn render_markdown(
    classification: &Classification,
    title: &str,
    marker: &str,
    show_unchanged: bool,
    gallery_url: Option<&str>,
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

    let mut wrote_section = false;
    for (status, heading) in [
        (Status::Added, "Added"),
        (Status::Changed, "Changed"),
        (Status::Removed, "Removed"),
    ] {
        wrote_section |= push_section(&mut md, classification, status, heading);
    }
    if show_unchanged {
        wrote_section |= push_section(&mut md, classification, Status::Unchanged, "Unchanged");
    }
    if !wrote_section {
        md.push_str("_No visual changes._\n");
    }

    if let Some(url) = gallery_url {
        md.push_str(&format!("\n[View gallery]({url})\n"));
    }

    md
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
        let md = render_markdown(
            &classification(),
            "Visual changes",
            "screencomp",
            false,
            None,
        );
        assert!(md.starts_with("<!-- screencomp -->\n"));
        assert!(md.contains("## Visual changes"));
        assert!(md.contains("| 1 | 1 | 0 | 4 |"));
        assert!(md.contains("### Added\n- `desktop/pricing`"));
        assert!(md.contains("### Changed\n- `desktop/home`"));
        assert!(!md.contains("### Removed"));
    }

    #[test]
    fn no_changes_message() {
        let empty = Classification {
            entries: vec![],
            counts: Counts::default(),
        };
        let md = render_markdown(&empty, "Visual changes", "screencomp", false, None);
        assert!(md.contains("_No visual changes._"));
    }

    #[test]
    fn appends_gallery_link() {
        let md = render_markdown(
            &classification(),
            "Visual changes",
            "screencomp",
            false,
            Some("https://example.test/g/"),
        );
        assert!(md.contains("[View gallery](https://example.test/g/)"));
    }
}
