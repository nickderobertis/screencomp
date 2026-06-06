//! Deterministic static-HTML gallery rendering.

use super::classify::{Classification, Status};
use super::snapshot::Snapshot;

/// Inline stylesheet kept tiny so the gallery is a single self-contained file.
const STYLE: &str = "body{font-family:system-ui,sans-serif;margin:2rem;color:#222}\
.grid{display:grid;grid-template-columns:repeat(auto-fill,minmax(240px,1fr));gap:1rem}\
section{margin-bottom:2rem}figure{margin:0}\
img{width:100%;height:auto;border:1px solid #ddd;border-radius:4px}\
figcaption{font-size:.85rem;color:#555;margin-top:.25rem}";

/// Render a self-contained HTML gallery for `snapshot`.
///
/// Variants are discovered from the snapshot's project grouping; image `src`
/// paths follow the `<project>/<name>.png` convention relative to the page, so
/// the gallery renders correctly when deployed alongside its images.
pub(crate) fn render_html(snapshot: &Snapshot, title: &str) -> String {
    let mut body = String::new();

    if snapshot.is_empty() {
        body.push_str("<p class=\"empty\">No screenshots.</p>\n");
    } else {
        let mut open_project: Option<&str> = None;
        for (key, _digest) in snapshot.iter() {
            if open_project != Some(key.project.as_str()) {
                if open_project.is_some() {
                    body.push_str("</div></section>\n");
                }
                body.push_str(&format!(
                    "<section><h2>{}</h2><div class=\"grid\">\n",
                    escape(&key.project)
                ));
                open_project = Some(key.project.as_str());
            }
            let src = escape(&format!("{}/{}.png", key.project, key.name));
            body.push_str(&format!(
                "<figure><img loading=\"lazy\" src=\"{src}\" alt=\"{src}\">\
<figcaption>{}</figcaption></figure>\n",
                escape(&key.name)
            ));
        }
        body.push_str("</div></section>\n");
    }

    format!(
        "<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n\
<meta charset=\"utf-8\">\n\
<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
<title>{title}</title>\n<style>{STYLE}</style>\n</head>\n<body>\n\
<h1>{title}</h1>\n{body}</body>\n</html>\n",
        title = escape(title),
    )
}

/// Inline stylesheet for the before/after diff gallery.
const DIFF_STYLE: &str = "body{font-family:system-ui,sans-serif;margin:2rem;color:#222}\
h1{margin-bottom:.5rem}section{margin:1.5rem 0}\
table.summary{border-collapse:collapse;margin:1rem 0}\
table.summary th,table.summary td{border:1px solid #ddd;padding:.3rem .8rem;text-align:right}\
.badge{font-size:.75rem;padding:.1rem .5rem;border-radius:999px;margin-left:.5rem;color:#fff;vertical-align:middle}\
.badge.changed{background:#b8860b}.badge.added{background:#2e7d32}\
.badge.removed{background:#c62828}.badge.unchanged{background:#888}\
figure{margin:0 0 1.5rem}figcaption{font-weight:600;margin-bottom:.4rem}\
.pair{display:grid;grid-template-columns:1fr 1fr;gap:1rem;max-width:760px}\
.lbl{display:block;font-size:.8rem;color:#666;margin-bottom:.25rem}\
img{width:100%;height:auto;border:1px solid #ddd;border-radius:4px}\
.single{max-width:380px}.unchanged img{opacity:.55}";

/// Render a before/after diff gallery from a `classification`.
///
/// Image `src` paths reference `baseline/<project>/<name>.png` and
/// `current/<project>/<name>.png`, which the gallery command copies alongside the
/// page. Changed shots show before and after side by side; added, removed, and
/// unchanged shots show a single image each.
pub(crate) fn render_diff_html(classification: &Classification, title: &str) -> String {
    let c = classification.counts;
    let mut body = format!(
        "<table class=\"summary\">\
<tr><th>Added</th><th>Changed</th><th>Removed</th><th>Unchanged</th></tr>\
<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr></table>\n",
        c.added, c.changed, c.removed, c.unchanged
    );

    if !classification.has_changes() {
        body.push_str("<p class=\"empty\">No visual changes.</p>\n");
    }

    // Changed: baseline (before) and current (after) side by side.
    let changed: Vec<_> = classification
        .entries
        .iter()
        .filter(|e| e.status == Status::Changed)
        .collect();
    if !changed.is_empty() {
        body.push_str("<section><h2>Changed</h2>\n");
        for e in changed {
            let label = format!("{}/{}", e.project, e.name);
            let before = format!("baseline/{}/{}.png", e.project, e.name);
            let after = format!("current/{}/{}.png", e.project, e.name);
            body.push_str(&format!(
                "<figure><figcaption>{}<span class=\"badge changed\">changed</span></figcaption>\
<div class=\"pair\">\
<div><span class=\"lbl\">before</span><img loading=\"lazy\" src=\"{}\" alt=\"{}\"></div>\
<div><span class=\"lbl\">after</span><img loading=\"lazy\" src=\"{}\" alt=\"{}\"></div>\
</div></figure>\n",
                escape(&label),
                escape(&before),
                escape(&label),
                escape(&after),
                escape(&label),
            ));
        }
        body.push_str("</section>\n");
    }

    // Added / Removed / Unchanged: one image each.
    body.push_str(&single_section(
        classification,
        Status::Added,
        "Added",
        "added",
        "current",
    ));
    body.push_str(&single_section(
        classification,
        Status::Removed,
        "Removed",
        "removed",
        "baseline",
    ));
    body.push_str(&single_section(
        classification,
        Status::Unchanged,
        "Unchanged",
        "unchanged",
        "current",
    ));

    format!(
        "<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n\
<meta charset=\"utf-8\">\n\
<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
<title>{title}</title>\n<style>{DIFF_STYLE}</style>\n</head>\n<body>\n\
<h1>{title}</h1>\n{body}</body>\n</html>\n",
        title = escape(title),
    )
}

/// Render a section of single-image figures for one `status`, sourcing images
/// from `dir` (`current` or `baseline`); empty string when there are none.
fn single_section(
    classification: &Classification,
    status: Status,
    heading: &str,
    badge: &str,
    dir: &str,
) -> String {
    let items: Vec<_> = classification
        .entries
        .iter()
        .filter(|e| e.status == status)
        .collect();
    if items.is_empty() {
        return String::new();
    }

    let mut section = format!("<section><h2>{heading}</h2>\n");
    for e in items {
        let label = format!("{}/{}", e.project, e.name);
        let src = format!("{}/{}/{}.png", dir, e.project, e.name);
        section.push_str(&format!(
            "<figure class=\"single {badge}\">\
<figcaption>{}<span class=\"badge {badge}\">{badge}</span></figcaption>\
<img loading=\"lazy\" src=\"{}\" alt=\"{}\"></figure>\n",
            escape(&label),
            escape(&src),
            escape(&label),
        ));
    }
    section.push_str("</section>\n");
    section
}

/// Escape the five characters that are unsafe in HTML text/attribute contexts.
fn escape(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::snapshot::ShotKey;

    fn snapshot() -> Snapshot {
        let mut s = Snapshot::new();
        for (project, name) in [
            ("desktop", "home"),
            ("desktop", "about"),
            ("mobile", "home"),
        ] {
            s.insert(
                ShotKey {
                    project: project.to_owned(),
                    name: name.to_owned(),
                },
                "deadbeef".to_owned(),
            );
        }
        s
    }

    #[test]
    fn renders_sorted_sections_and_images() {
        let html = render_html(&snapshot(), "Demo");
        assert!(html.starts_with("<!DOCTYPE html>"));
        assert!(html.contains("<title>Demo</title>"));
        assert!(html.contains("<h2>desktop</h2>"));
        assert!(html.contains("src=\"desktop/about.png\""));
        assert!(html.contains("src=\"mobile/home.png\""));
        // desktop sorts before mobile; about before home within desktop.
        let about = html.find("desktop/about.png").unwrap();
        let mobile = html.find("mobile/home.png").unwrap();
        assert!(about < mobile);
    }

    #[test]
    fn escapes_unsafe_characters() {
        let html = render_html(&snapshot(), "<script>");
        assert!(html.contains("<title>&lt;script&gt;</title>"));
        assert!(!html.contains("<title><script>"));
    }

    #[test]
    fn empty_snapshot_renders_placeholder() {
        let html = render_html(&Snapshot::new(), "Empty");
        assert!(html.contains("No screenshots."));
    }

    #[test]
    fn diff_renders_before_after_and_status_sections() {
        use crate::domain::classify::{Classification, Counts, Entry, Status};

        let entry = |project: &str, name: &str, status| Entry {
            project: project.to_owned(),
            name: name.to_owned(),
            status,
        };
        let classification = Classification {
            entries: vec![
                entry("desktop", "about", Status::Changed),
                entry("desktop", "home", Status::Unchanged),
                entry("desktop", "pricing", Status::Added),
                entry("mobile", "home", Status::Removed),
            ],
            counts: Counts {
                added: 1,
                changed: 1,
                removed: 1,
                unchanged: 1,
            },
        };

        let html = render_diff_html(&classification, "Diff");
        assert!(html.contains("<title>Diff</title>"));
        // Changed shows both sides.
        assert!(html.contains("<h2>Changed</h2>"));
        assert!(html.contains("src=\"baseline/desktop/about.png\""));
        assert!(html.contains("src=\"current/desktop/about.png\""));
        // Added from current, removed from baseline.
        assert!(html.contains("<h2>Added</h2>"));
        assert!(html.contains("src=\"current/desktop/pricing.png\""));
        assert!(html.contains("<h2>Removed</h2>"));
        assert!(html.contains("src=\"baseline/mobile/home.png\""));
        assert!(html.contains("<h2>Unchanged</h2>"));
    }

    #[test]
    fn diff_with_no_changes_says_so() {
        use crate::domain::classify::{Classification, Counts};

        let classification = Classification {
            entries: Vec::new(),
            counts: Counts::default(),
        };
        let html = render_diff_html(&classification, "Diff");
        assert!(html.contains("No visual changes."));
    }
}
