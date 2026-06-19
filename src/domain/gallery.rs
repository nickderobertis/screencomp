//! Deterministic static-HTML gallery rendering.
//!
//! The plain gallery renders one card per screenshot *name*, with a control group
//! per declared toggle dimension (theme, viewport, …). Selecting a toggle swaps
//! which captured image is shown, so a screen that varies across several settings
//! is one focused card you toggle through rather than a wall of near-duplicate
//! thumbnails. The page is a single self-contained file: inline CSS, a tiny inline
//! script, and `src` paths relative to the page so it deploys as-is. The default
//! variant is marked visible server-side, so the gallery still shows something
//! without JavaScript.

use super::classify::{Classification, Status};
use super::snapshot::{Shot, ShotKey, Snapshot};
use super::toggle::ToggleDim;

/// Inline stylesheet kept tiny so the gallery is a single self-contained file.
const STYLE: &str = "body{font-family:system-ui,sans-serif;margin:2rem;color:#222}\
section.shot{margin-bottom:2.5rem}h2{margin-bottom:.5rem}\
.toggles{display:flex;flex-wrap:wrap;gap:1rem;margin:.25rem 0 .75rem}\
.toggle{display:flex;align-items:center;gap:.4rem}\
.dim-label{font-size:.8rem;color:#555}\
.toggle button{font:inherit;font-size:.8rem;padding:.15rem .6rem;border:1px solid #bbb;\
background:#f6f6f6;border-radius:999px;cursor:pointer}\
.toggle button.active{background:#222;color:#fff;border-color:#222}\
.variants img{max-width:100%;height:auto;border:1px solid #ddd;border-radius:4px}\
.variant[hidden]{display:none}";

/// Inline script: per card, track the selected value of each toggle dimension and
/// show the one image whose `data-variant` matches. Kept dependency-free and
/// deterministic (a static string).
const SCRIPT: &str = "for(const card of document.querySelectorAll('.shot')){\
const sel={};\
for(const t of card.querySelectorAll('.toggle')){\
const a=t.querySelector('button.active')||t.querySelector('button');\
if(a)sel[t.dataset.dim]=a.dataset.val;}\
const key=()=>Object.keys(sel).sort().map(k=>k+'='+sel[k]).join(';');\
const apply=()=>{const want=key();\
for(const img of card.querySelectorAll('.variant'))img.hidden=img.dataset.variant!==want;\
for(const b of card.querySelectorAll('.toggle button'))\
b.classList.toggle('active',sel[b.parentElement.dataset.dim]===b.dataset.val);};\
for(const b of card.querySelectorAll('.toggle button'))\
b.addEventListener('click',()=>{sel[b.parentElement.dataset.dim]=b.dataset.val;apply();});\
apply();}";

/// Render a self-contained HTML gallery for `snapshot`, with toggle controls
/// driven by the declared `dims`.
pub(crate) fn render_html(snapshot: &Snapshot, dims: &[ToggleDim], title: &str) -> String {
    let mut body = String::new();
    if snapshot.is_empty() {
        body.push_str("<p class=\"empty\">No screenshots.</p>\n");
    } else {
        for group in group_by_name(snapshot) {
            push_card(&mut body, &group, dims);
        }
    }

    format!(
        "<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n\
<meta charset=\"utf-8\">\n\
<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
<title>{title}</title>\n<style>{STYLE}</style>\n</head>\n<body>\n\
<h1>{title}</h1>\n{body}<script>{SCRIPT}</script>\n</body>\n</html>\n",
        title = escape(title),
    )
}

/// One screenshot name and the shots captured for it, in toggle order.
struct Group<'a> {
    name: &'a str,
    shots: Vec<(&'a ShotKey, &'a Shot)>,
}

/// Group a snapshot's shots by name, preserving `(name, toggles)` order.
fn group_by_name(snapshot: &Snapshot) -> Vec<Group<'_>> {
    let mut groups: Vec<Group<'_>> = Vec::new();
    for (key, shot) in snapshot.iter() {
        match groups.last_mut() {
            Some(g) if g.name == key.name => g.shots.push((key, shot)),
            _ => groups.push(Group {
                name: &key.name,
                shots: vec![(key, shot)],
            }),
        }
    }
    groups
}

/// A toggle dimension that actually distinguishes a name's shots: it has at least
/// two of its declared values present, so it is worth a control. `values` holds
/// only the present values, in the dimension's declared order.
struct Control<'a> {
    key: &'a str,
    label: &'a str,
    values: Vec<&'a str>,
}

/// The control dimensions for `group`: declared dimensions (in config order) whose
/// key appears with two or more distinct values across the group's shots.
fn controls_for<'a>(group: &Group<'a>, dims: &'a [ToggleDim]) -> Vec<Control<'a>> {
    dims.iter()
        .filter_map(|dim| {
            let present: Vec<&str> = dim
                .values
                .iter()
                .map(String::as_str)
                .filter(|v| {
                    group.shots.iter().any(|(k, _)| {
                        k.toggles.get(dim.key.as_str()).map(String::as_str) == Some(*v)
                    })
                })
                .collect();
            (present.len() >= 2).then(|| Control {
                key: &dim.key,
                label: &dim.label,
                values: present,
            })
        })
        .collect()
}

/// The `data-variant`/selection key for a shot over `controls`: `key=value` pairs
/// in control-key order (sorted, matching the inline script's `Object.keys().sort()`).
/// A control dimension absent from the shot contributes an empty value.
fn variant_key(toggles: &super::snapshot::Toggles, controls: &[Control<'_>]) -> String {
    let mut keys: Vec<&str> = controls.iter().map(|c| c.key).collect();
    keys.sort_unstable();
    keys.iter()
        .map(|k| format!("{k}={}", toggles.get(*k).map(String::as_str).unwrap_or("")))
        .collect::<Vec<_>>()
        .join(";")
}

/// Append one screenshot card: heading, toggle controls, and the image variants.
fn push_card(body: &mut String, group: &Group<'_>, dims: &[ToggleDim]) {
    let controls = controls_for(group, dims);

    body.push_str(&format!(
        "<section class=\"shot\"><h2>{}</h2>\n",
        escape(group.name)
    ));

    // Default selection: the first present value of each control (declared order),
    // which fixes which variant is visible without JavaScript.
    let mut default = super::snapshot::Toggles::new();
    for c in &controls {
        if let Some(first) = c.values.first() {
            default.insert(c.key.to_owned(), (*first).to_owned());
        }
    }
    let default_key = variant_key(&default, &controls);

    if !controls.is_empty() {
        body.push_str("<div class=\"toggles\">");
        for c in &controls {
            body.push_str(&format!(
                "<div class=\"toggle\" data-dim=\"{}\"><span class=\"dim-label\">{}</span>",
                escape(c.key),
                escape(c.label)
            ));
            for v in &c.values {
                let active = default.get(c.key).map(String::as_str) == Some(*v);
                body.push_str(&format!(
                    "<button type=\"button\" data-val=\"{}\"{}>{}</button>",
                    escape(v),
                    if active { " class=\"active\"" } else { "" },
                    escape(v),
                ));
            }
            body.push_str("</div>");
        }
        body.push_str("</div>\n");
    }

    body.push_str("<div class=\"variants\">");
    for (key, shot) in &group.shots {
        let Some(image) = shot.image.as_deref() else {
            continue; // a digest-only shot has no image to show
        };
        let variant = variant_key(&key.toggles, &controls);
        let hidden = if variant == default_key {
            ""
        } else {
            " hidden"
        };
        let src = escape(image);
        body.push_str(&format!(
            "<img class=\"variant\" data-variant=\"{}\" loading=\"lazy\" src=\"{src}\" alt=\"{}\"{hidden}>",
            escape(&variant),
            escape(&key.label()),
        ));
    }
    body.push_str("</div></section>\n");
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
.single{max-width:380px}.unchanged img{opacity:.55}.missing{color:#888;font-style:italic}";

/// Render a before/after diff gallery from a `classification`.
///
/// Each shot is labeled by its name and toggles. Changed shots show baseline and
/// current side by side (sourced from `baseline/<image>` and `current/<image>`,
/// which the gallery command copies alongside the page); added, removed, and
/// unchanged shots show a single image. A side without an image (e.g. a
/// manifest-mode baseline) renders a "no image" note rather than a broken `<img>`.
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

    let changed: Vec<_> = classification
        .entries
        .iter()
        .filter(|e| e.status == Status::Changed)
        .collect();
    if !changed.is_empty() {
        body.push_str("<section><h2>Changed</h2>\n");
        for e in changed {
            let label = e.key.label();
            body.push_str(&format!(
                "<figure><figcaption>{}<span class=\"badge changed\">changed</span></figcaption>\
<div class=\"pair\">\
<div><span class=\"lbl\">before</span>{}</div>\
<div><span class=\"lbl\">after</span>{}</div>\
</div></figure>\n",
                escape(&label),
                side_img(e.baseline_image.as_deref(), "baseline", &label),
                side_img(e.current_image.as_deref(), "current", &label),
            ));
        }
        body.push_str("</section>\n");
    }

    body.push_str(&single_section(
        classification,
        Status::Added,
        "Added",
        "added",
    ));
    body.push_str(&single_section(
        classification,
        Status::Removed,
        "Removed",
        "removed",
    ));
    body.push_str(&single_section(
        classification,
        Status::Unchanged,
        "Unchanged",
        "unchanged",
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

/// Render one side of a diff figure: an `<img>` under `dir/<image>`, or a "no
/// image" note when that side has no image (manifest-mode baseline).
fn side_img(image: Option<&str>, dir: &str, label: &str) -> String {
    match image {
        Some(image) => {
            let src = escape(&format!("{dir}/{image}"));
            format!(
                "<img loading=\"lazy\" src=\"{src}\" alt=\"{}\">",
                escape(label)
            )
        }
        None => "<span class=\"missing\">no image</span>".to_owned(),
    }
}

/// Render a section of single-image figures for one `status`. Added and unchanged
/// shots source from `current/<image>`; removed shots from `baseline/<image>`.
/// Empty string when there are none.
fn single_section(
    classification: &Classification,
    status: Status,
    heading: &str,
    badge: &str,
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
        let label = e.key.label();
        let (image, dir) = if status == Status::Removed {
            (e.baseline_image.as_deref(), "baseline")
        } else {
            (e.current_image.as_deref(), "current")
        };
        section.push_str(&format!(
            "<figure class=\"single {badge}\">\
<figcaption>{}<span class=\"badge {badge}\">{badge}</span></figcaption>{}</figure>\n",
            escape(&label),
            side_img(image, dir, &label),
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
    use crate::domain::classify::{Counts, Entry};

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

    fn snapshot() -> Snapshot {
        let mut s = Snapshot::new();
        for (name, theme) in [("home", "light"), ("home", "dark"), ("about", "light")] {
            let key = ShotKey::with(name, &[("theme", theme)]);
            s.insert(
                key.clone(),
                Shot::new("deadbeef".to_owned(), Some(format!("{name}-{theme}.png"))),
            );
        }
        s
    }

    #[test]
    fn renders_one_card_per_name_with_toggle_controls() {
        let html = render_html(&snapshot(), &dims(), "Demo");
        assert!(html.starts_with("<!DOCTYPE html>"));
        assert!(html.contains("<title>Demo</title>"));
        // One card each for about and home; home carries a theme toggle (2 values).
        assert!(html.contains("<h2>about</h2>"));
        assert!(html.contains("<h2>home</h2>"));
        assert!(html.contains("data-dim=\"theme\""));
        // viewport never appears in the shots, so it gets no control.
        assert!(!html.contains("data-dim=\"viewport\""));
        // The two home variants are present and keyed by their toggle value.
        assert!(html.contains("data-variant=\"theme=light\""));
        assert!(html.contains("data-variant=\"theme=dark\""));
        assert!(html.contains("src=\"home-dark.png\""));
        // about sorts before home.
        assert!(html.find("<h2>about</h2>").unwrap() < html.find("<h2>home</h2>").unwrap());
        // The interactive script is inlined.
        assert!(html.contains("querySelectorAll('.shot')"));
    }

    #[test]
    fn default_variant_is_visible_and_others_hidden() {
        let html = render_html(&snapshot(), &dims(), "Demo");
        // light is the first declared theme value, so it is the default (no hidden).
        assert!(html.contains("data-variant=\"theme=light\" loading=\"lazy\" src=\"home-light.png\" alt=\"home [theme=light]\">"));
        // dark is hidden by default.
        assert!(html.contains("data-variant=\"theme=dark\" loading=\"lazy\" src=\"home-dark.png\" alt=\"home [theme=dark]\" hidden>"));
        // The default theme button is marked active.
        assert!(html.contains("data-val=\"light\" class=\"active\">light</button>"));
    }

    #[test]
    fn single_shot_name_has_no_controls() {
        let mut s = Snapshot::new();
        s.insert(
            ShotKey::bare("home"),
            Shot::new("aa", Some("home.png".to_owned())),
        );
        let html = render_html(&s, &dims(), "Solo");
        assert!(!html.contains("class=\"toggles\""));
        assert!(html.contains("data-variant=\"\" loading=\"lazy\" src=\"home.png\""));
    }

    #[test]
    fn escapes_unsafe_characters() {
        let html = render_html(&snapshot(), &dims(), "<script>");
        assert!(html.contains("<title>&lt;script&gt;</title>"));
    }

    #[test]
    fn empty_snapshot_renders_placeholder() {
        let html = render_html(&Snapshot::new(), &dims(), "Empty");
        assert!(html.contains("No screenshots."));
    }

    fn entry(name: &str, toggles: &[(&str, &str)], status: Status) -> Entry {
        Entry {
            key: ShotKey::with(name, toggles),
            status,
            baseline_image: Some(format!("{name}.png")),
            current_image: Some(format!("{name}.png")),
        }
    }

    #[test]
    fn diff_renders_before_after_and_status_sections() {
        let classification = Classification {
            entries: vec![
                entry("about", &[("theme", "dark")], Status::Changed),
                entry("home", &[("theme", "dark")], Status::Unchanged),
                entry("pricing", &[("theme", "dark")], Status::Added),
                Entry {
                    status: Status::Removed,
                    current_image: None,
                    ..entry("legacy", &[("theme", "dark")], Status::Removed)
                },
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
        assert!(html.contains("<h2>Changed</h2>"));
        assert!(html.contains("about [theme=dark]"));
        assert!(html.contains("src=\"baseline/about.png\""));
        assert!(html.contains("src=\"current/about.png\""));
        assert!(html.contains("<h2>Added</h2>"));
        assert!(html.contains("src=\"current/pricing.png\""));
        assert!(html.contains("<h2>Removed</h2>"));
        assert!(html.contains("src=\"baseline/legacy.png\""));
        assert!(html.contains("<h2>Unchanged</h2>"));
    }

    #[test]
    fn diff_side_without_image_shows_note() {
        let classification = Classification {
            entries: vec![Entry {
                key: ShotKey::bare("home"),
                status: Status::Changed,
                baseline_image: None, // manifest-mode baseline
                current_image: Some("home.png".to_owned()),
            }],
            counts: Counts {
                changed: 1,
                ..Counts::default()
            },
        };
        let html = render_diff_html(&classification, "Diff");
        assert!(html.contains("no image"));
        assert!(html.contains("src=\"current/home.png\""));
    }

    #[test]
    fn diff_with_no_changes_says_so() {
        let classification = Classification {
            entries: vec![],
            counts: Counts::default(),
        };
        let html = render_diff_html(&classification, "Diff");
        assert!(html.contains("No visual changes."));
    }
}
