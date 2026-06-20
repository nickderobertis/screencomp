//! Deterministic static-HTML gallery rendering.
//!
//! The plain gallery renders one *toggle bar* at the top of the page — one control
//! group per declared dimension (theme, viewport, …) that distinguishes the
//! capture's shots — and below it one card per screenshot *name*. The bar is the
//! single, page-wide selection: changing it filters every card at once, showing
//! the one image per name that matches the selection and hiding names that have no
//! matching shot. So a capture that varies across several settings is one set of
//! controls over a focused, filtered list rather than a wall of near-duplicate
//! thumbnails or a control group repeated on every card. The page is a single
//! self-contained file: inline CSS, a tiny inline script, and `src` paths relative
//! to the page so it deploys as-is. The default selection is applied server-side,
//! so the gallery still shows something without JavaScript.

use super::classify::{Classification, Status};
use super::snapshot::{Shot, ShotKey, Snapshot};
use super::toggle::ToggleDim;

/// Inline stylesheet kept tiny so the gallery is a single self-contained file.
const STYLE: &str = "body{font-family:system-ui,sans-serif;margin:2rem;color:#222}\
section.shot{margin-bottom:2.5rem}section.shot[hidden]{display:none}h2{margin-bottom:.5rem}\
.toggles{display:flex;flex-wrap:wrap;gap:1rem;margin:0 0 1.5rem;position:sticky;top:0;\
background:#fff;padding:.75rem 0;z-index:1}\
.toggle{display:flex;align-items:center;gap:.4rem}\
.dim-label{font-size:.8rem;color:#555}\
.toggle button{font:inherit;font-size:.8rem;padding:.15rem .6rem;border:1px solid #bbb;\
background:#f6f6f6;border-radius:999px;cursor:pointer}\
.toggle button.active{background:#222;color:#fff;border-color:#222}\
.variants img{max-width:100%;height:auto;border:1px solid #ddd;border-radius:4px}\
.variant[hidden]{display:none}";

/// Inline script: read the page-wide selection from the top toggle bar, then show
/// the one image per card whose toggles match it and hide cards with no match.
/// An image matches when every toggle it carries equals the selected value for
/// that dimension; dimensions the image lacks are wildcards. Cards with no images
/// (digest-only names) are left untouched. Kept dependency-free and deterministic
/// (a static string).
const SCRIPT: &str = "const bar=document.querySelector('.toggles');\
if(bar){const sel={};\
for(const t of bar.querySelectorAll('.toggle')){\
const a=t.querySelector('button.active')||t.querySelector('button');\
if(a)sel[t.dataset.dim]=a.dataset.val;}\
const parse=s=>{const o={};if(s)for(const p of s.split(';')){\
const i=p.indexOf('=');o[p.slice(0,i)]=p.slice(i+1);}return o;};\
const match=v=>{for(const k in v)if(sel[k]!==v[k])return false;return true;};\
const apply=()=>{\
for(const sec of document.querySelectorAll('.shot')){\
const imgs=sec.querySelectorAll('.variant');if(!imgs.length)continue;\
let any=false;\
for(const img of imgs){const ok=match(parse(img.dataset.variant));img.hidden=!ok;if(ok)any=true;}\
sec.hidden=!any;}\
for(const b of bar.querySelectorAll('.toggle button'))\
b.classList.toggle('active',sel[b.parentElement.dataset.dim]===b.dataset.val);};\
for(const b of bar.querySelectorAll('.toggle button'))\
b.addEventListener('click',()=>{sel[b.parentElement.dataset.dim]=b.dataset.val;apply();});\
apply();}";

/// Render a self-contained HTML gallery for `snapshot`, with toggle controls
/// driven by the declared `dims`.
pub(crate) fn render_html(snapshot: &Snapshot, dims: &[ToggleDim], title: &str) -> String {
    let mut body = String::new();
    if snapshot.is_empty() {
        body.push_str("<p class=\"empty\">No screenshots.</p>\n");
    } else {
        // One page-wide control bar over every dimension that distinguishes the
        // capture's shots, plus the default selection it starts on.
        let controls = controls_for(snapshot, dims);
        let default = default_selection(&controls);
        push_toggle_bar(&mut body, &controls, &default);
        for group in group_by_name(snapshot) {
            push_card(&mut body, &group, &controls, &default);
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

/// The page-wide control dimensions: declared dimensions (in config order) whose
/// key appears with two or more distinct values across *all* of the capture's
/// shots. These drive the single top toggle bar that filters every card.
fn controls_for<'a>(snapshot: &'a Snapshot, dims: &'a [ToggleDim]) -> Vec<Control<'a>> {
    dims.iter()
        .filter_map(|dim| {
            let present: Vec<&str> = dim
                .values
                .iter()
                .map(String::as_str)
                .filter(|v| {
                    snapshot.iter().any(|(k, _)| {
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

/// The default page selection: the first present value of each control (declared
/// order), which fixes which images are visible without JavaScript.
fn default_selection(controls: &[Control<'_>]) -> super::snapshot::Toggles {
    let mut default = super::snapshot::Toggles::new();
    for c in controls {
        if let Some(first) = c.values.first() {
            default.insert(c.key.to_owned(), (*first).to_owned());
        }
    }
    default
}

/// The `data-variant` key for a shot over `controls`: `key=value` pairs for only
/// the control dimensions the shot *carries*, in sorted key order. Dimensions the
/// shot lacks are omitted, so they act as wildcards when matched against the
/// selection (matching the inline script's `match`).
fn variant_key(toggles: &super::snapshot::Toggles, controls: &[Control<'_>]) -> String {
    let mut pairs: Vec<(&str, &str)> = controls
        .iter()
        .filter_map(|c| toggles.get(c.key).map(|v| (c.key, v.as_str())))
        .collect();
    pairs.sort_unstable_by_key(|(k, _)| *k);
    pairs
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join(";")
}

/// Whether a shot is visible under `selection`: every control dimension it carries
/// must equal the selected value. Dimensions it lacks are wildcards. Mirrors the
/// inline script so server-side default visibility matches client-side filtering.
fn matches(
    toggles: &super::snapshot::Toggles,
    controls: &[Control<'_>],
    selection: &super::snapshot::Toggles,
) -> bool {
    controls
        .iter()
        .filter_map(|c| toggles.get(c.key).map(|v| (c.key, v)))
        .all(|(k, v)| selection.get(k) == Some(v))
}

/// Append the single page-wide toggle bar with the default value of each control
/// marked active. Nothing is emitted when there are no controls.
fn push_toggle_bar(
    body: &mut String,
    controls: &[Control<'_>],
    default: &super::snapshot::Toggles,
) {
    if controls.is_empty() {
        return;
    }
    body.push_str("<div class=\"toggles\">");
    for c in controls {
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

/// Append one screenshot card: a heading and the name's image variants, each keyed
/// by its toggles so the top bar can filter them. The card is hidden server-side
/// when it has images but none match the default selection; a digest-only name
/// (no images) is always shown.
fn push_card(
    body: &mut String,
    group: &Group<'_>,
    controls: &[Control<'_>],
    default: &super::snapshot::Toggles,
) {
    let mut variants = String::new();
    let mut has_image = false;
    let mut any_visible = false;
    for (key, shot) in &group.shots {
        let Some(image) = shot.image.as_deref() else {
            continue; // a digest-only shot has no image to show
        };
        has_image = true;
        let visible = matches(&key.toggles, controls, default);
        any_visible |= visible;
        variants.push_str(&format!(
            "<img class=\"variant\" data-variant=\"{}\" loading=\"lazy\" src=\"{}\" alt=\"{}\"{}>",
            escape(&variant_key(&key.toggles, controls)),
            escape(image),
            escape(&key.label()),
            if visible { "" } else { " hidden" },
        ));
    }

    let hidden = if has_image && !any_visible {
        " hidden"
    } else {
        ""
    };
    body.push_str(&format!(
        "<section class=\"shot\"{hidden}><h2>{}</h2>\n<div class=\"variants\">{variants}</div></section>\n",
        escape(group.name),
    ));
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
    fn digest_only_name_renders_and_is_never_filtered_out() {
        // A name whose only shot has no image (a digest-only/manifest shot) still
        // gets a card with its heading, and is not hidden even though it has no
        // image to match the selection.
        let mut s = Snapshot::new();
        s.insert(
            ShotKey::with("home", &[("theme", "light")]),
            Shot::new("aa", Some("home-light.png".to_owned())),
        );
        s.insert(
            ShotKey::with("home", &[("theme", "dark")]),
            Shot::new("bb", Some("home-dark.png".to_owned())),
        );
        s.insert(ShotKey::bare("digest"), Shot::new("cc", None));
        let html = render_html(&s, &dims(), "Demo");
        // The digest-only card is present and not hidden, with no image variant.
        assert!(
            html.contains(
                "<section class=\"shot\"><h2>digest</h2>\n<div class=\"variants\"></div>"
            ),
            "{html}"
        );
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
    fn one_toggle_bar_filters_every_card() {
        let mut s = Snapshot::new();
        for (name, theme) in [("home", "light"), ("home", "dark"), ("nightly", "dark")] {
            s.insert(
                ShotKey::with(name, &[("theme", theme)]),
                Shot::new("aa", Some(format!("{name}-{theme}.png"))),
            );
        }
        let html = render_html(&s, &dims(), "Demo");
        // Exactly one page-wide toggle bar, not one repeated per card.
        assert_eq!(html.matches("class=\"toggles\"").count(), 1, "{html}");
        assert!(html.contains("data-dim=\"theme\""));
        // `nightly` only carries the non-default `dark`, so under the default
        // `light` selection its card is filtered out (hidden) server-side.
        assert!(
            html.contains("<section class=\"shot\" hidden><h2>nightly</h2>"),
            "{html}"
        );
        // `home` has a matching default variant, so its card stays visible.
        assert!(
            html.contains("<section class=\"shot\"><h2>home</h2>"),
            "{html}"
        );
    }

    #[test]
    fn card_missing_a_dimension_is_a_wildcard() {
        let mut s = Snapshot::new();
        // Both theme and viewport vary across the capture, so both are controls.
        s.insert(
            ShotKey::with("home", &[("theme", "light"), ("viewport", "desktop")]),
            Shot::new("a", Some("h-ld.png".to_owned())),
        );
        s.insert(
            ShotKey::with("home", &[("theme", "dark"), ("viewport", "mobile")]),
            Shot::new("a", Some("h-dm.png".to_owned())),
        );
        // `about` carries only theme; the absent viewport acts as a wildcard.
        s.insert(
            ShotKey::with("about", &[("theme", "light")]),
            Shot::new("a", Some("about.png".to_owned())),
        );
        let html = render_html(&s, &dims(), "Demo");
        assert!(html.contains("data-dim=\"theme\""));
        assert!(html.contains("data-dim=\"viewport\""));
        // about's variant key omits the dimension it lacks.
        assert!(
            html.contains("data-variant=\"theme=light\" loading=\"lazy\" src=\"about.png\""),
            "{html}"
        );
        // Default selection is theme=light, viewport=desktop; about matches on
        // theme and wildcards viewport, so its card is visible.
        assert!(
            html.contains("<section class=\"shot\"><h2>about</h2>"),
            "{html}"
        );
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
