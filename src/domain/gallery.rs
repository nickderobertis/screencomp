//! Deterministic static-HTML gallery rendering.

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
}
