//! `screencomp comment` — render the sticky pull-request comment body.

use std::io::Write;

use camino::{Utf8Path, Utf8PathBuf};

use super::{Ctx, baseline_snapshot, discover_scoped, resolve_arch, write_err};
use crate::cli::CommentArgs;
use crate::config::Config;
use crate::domain::classify::classify;
use crate::domain::comment::{
    ImageBases, ProjectSummary, render_aggregated_markdown, render_markdown,
};
use crate::errors::AppError;
use crate::io::fs;

/// Default marker for the single aggregated comment. Distinct from the
/// per-project markers (`screencomp-<project>-<arch>`) so the aggregated surface
/// never clobbers, or is clobbered by, a per-project one.
const AGGREGATE_MARKER: &str = "screencomp-aggregate";

/// Only-understood version of the `--projects` spec contract. Bump deliberately:
/// a new schema is a new contract with the workflow that generates the spec.
const PROJECTS_SPEC_SCHEMA: u32 = 1;

pub(crate) fn run(args: &CommentArgs, ctx: &Ctx, out: &mut dyn Write) -> Result<i32, AppError> {
    // Config is loaded once at the run boundary and shared via Ctx.
    let cfg = &ctx.config;

    // Aggregated mode renders one combined comment across many projects; it shares
    // nothing with the single-project path but the marker/title/output plumbing.
    if let Some(spec) = args.projects.as_deref() {
        return run_aggregated(spec, args, cfg, ctx, out);
    }

    let arch = resolve_arch(args.arch.as_deref(), &cfg.capture.arches)?;
    let plat = arch.as_deref();
    let manifest_mode = args.baseline_manifest.is_some();
    let baseline = baseline_snapshot(
        args.baseline.as_deref(),
        args.baseline_manifest.as_deref(),
        plat,
    )?;
    // clap requires `--current` in every path that reaches here (`required_unless_present`).
    let current_root = args
        .current
        .as_deref()
        .expect("clap requires --current when --projects is absent");
    let current = discover_scoped(current_root, plat)?;
    let classification = classify(&baseline, &current, &[]);

    // CLI flags override the configured values when present.
    let embed_limit = args.embed_limit.unwrap_or(cfg.comment.embed_limit);
    let title = args.title.as_deref().unwrap_or(&cfg.comment.title);
    let marker = args.marker.as_deref().unwrap_or(&cfg.comment.marker);

    // Resolve where the "Before"/"After" images are hosted. Explicit flags win;
    // otherwise `--gallery-url` derives them from the layout `gallery` writes,
    // which differs by baseline mode (see `image_bases`).
    let (before, after) = image_bases(args, manifest_mode);
    let markdown = render_markdown(
        &classification,
        title,
        marker,
        cfg.comment.show_unchanged,
        args.gallery_url.as_deref(),
        ImageBases {
            before: before.as_deref(),
            after: after.as_deref(),
        },
        embed_limit,
    );

    emit(&markdown, args.output.as_deref(), ctx, out)
}

/// Write the rendered comment to `--output` (reporting the path unless quiet) or
/// to stdout, the shared tail of both the per-project and aggregated paths.
fn emit(
    markdown: &str,
    output: Option<&Utf8Path>,
    ctx: &Ctx,
    out: &mut dyn Write,
) -> Result<i32, AppError> {
    match output {
        Some(path) => {
            fs::write_string(path, markdown)?;
            if !ctx.quiet {
                writeln!(out, "wrote {path}").map_err(write_err)?;
            }
        }
        None => write!(out, "{markdown}").map_err(write_err)?,
    }
    Ok(0)
}

/// Render ONE aggregated comment covering every project in the `--projects` spec.
///
/// Each project is classified independently (its own baseline, current capture,
/// and arch), then reduced to a single summary row. Only the projects listed
/// appear, so an affected-only monorepo run naturally omits unaffected projects.
fn run_aggregated(
    spec_path: &Utf8Path,
    args: &CommentArgs,
    cfg: &Config,
    ctx: &Ctx,
    out: &mut dyn Write,
) -> Result<i32, AppError> {
    let spec = read_projects_spec(spec_path)?;

    // Classify every project first, owning the label/counts/url so the borrowed
    // `ProjectSummary` view can reference them when rendering.
    let mut rows: Vec<(String, crate::domain::classify::Counts, Option<String>)> =
        Vec::with_capacity(spec.projects.len());
    for project in &spec.projects {
        let arch = resolve_arch(project.arch.as_deref(), &cfg.capture.arches)?;
        let plat = arch.as_deref();
        let baseline = baseline_snapshot(
            project.baseline.as_deref(),
            project.baseline_manifest.as_deref(),
            plat,
        )?;
        let current = discover_scoped(&project.current, plat)?;
        let counts = classify(&baseline, &current, &[]).counts;
        let label = project.label.clone().unwrap_or_else(|| project.id.clone());
        rows.push((label, counts, project.gallery_url.clone()));
    }

    let summaries: Vec<ProjectSummary<'_>> = rows
        .iter()
        .map(|(label, counts, url)| ProjectSummary {
            label,
            counts: *counts,
            gallery_url: url.as_deref(),
        })
        .collect();

    let title = args.title.as_deref().unwrap_or(&cfg.comment.title);
    let marker = args.marker.as_deref().unwrap_or(AGGREGATE_MARKER);
    let markdown = render_aggregated_markdown(&summaries, title, marker);

    emit(&markdown, args.output.as_deref(), ctx, out)
}

/// The `--projects` spec: a versioned JSON document naming the projects to fold
/// into one aggregated comment.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ProjectsSpec {
    /// Contract version; only [`PROJECTS_SPEC_SCHEMA`] is understood.
    schema: u32,
    /// The affected projects, each rendered as one row.
    projects: Vec<ProjectEntry>,
}

/// One project in a [`ProjectsSpec`]: its identity plus the same inputs the
/// single-project `comment` path takes, so each row classifies the same way.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ProjectEntry {
    /// Stable project ID; also the default display label.
    id: String,
    /// Display label for the row; defaults to `id`.
    label: Option<String>,
    /// Current capture root (`<dir>/[<arch>/]captures.json`).
    current: Utf8PathBuf,
    /// Baseline image tree. Exactly one of `baseline`/`baseline_manifest` is set.
    baseline: Option<Utf8PathBuf>,
    /// Baseline digest manifest. Exactly one of `baseline`/`baseline_manifest` is set.
    baseline_manifest: Option<Utf8PathBuf>,
    /// CPU-arch subtree to scope to; resolved like `--arch` when omitted.
    arch: Option<String>,
    /// Per-project gallery URL linked from the row.
    gallery_url: Option<String>,
}

/// Read and validate the `--projects` spec, returning a typed error for a missing
/// file, malformed JSON, an unknown schema, an empty/duplicate ID set, or a
/// project without exactly one baseline source.
fn read_projects_spec(path: &Utf8Path) -> Result<ProjectsSpec, AppError> {
    let text = fs::read_text(path)?;
    let spec: ProjectsSpec = serde_json::from_str(&text).map_err(|e| AppError::InvalidLayout {
        path: path.to_owned(),
        reason: e.to_string(),
    })?;
    let invalid = |reason: String| AppError::InvalidLayout {
        path: path.to_owned(),
        reason,
    };
    if spec.schema != PROJECTS_SPEC_SCHEMA {
        return Err(invalid(format!(
            "unsupported projects spec schema {} (this screencomp understands {PROJECTS_SPEC_SCHEMA})",
            spec.schema
        )));
    }
    let mut seen = std::collections::BTreeSet::new();
    for project in &spec.projects {
        if project.id.is_empty() {
            return Err(invalid("each project needs a non-empty `id`".to_owned()));
        }
        if !seen.insert(project.id.as_str()) {
            return Err(invalid(format!("duplicate project id `{}`", project.id)));
        }
        match (&project.baseline, &project.baseline_manifest) {
            (Some(_), Some(_)) => {
                return Err(invalid(format!(
                    "project `{}` sets both `baseline` and `baseline_manifest`; use exactly one",
                    project.id
                )));
            }
            (None, None) => {
                return Err(invalid(format!(
                    "project `{}` needs a `baseline` or `baseline_manifest`",
                    project.id
                )));
            }
            _ => {}
        }
    }
    Ok(spec)
}

/// Resolve the `(before, after)` image bases for the inline previews.
///
/// `--baseline-url`/`--current-url` are used verbatim when set. Otherwise
/// `--gallery-url U` derives them from the layout `gallery` produces — which
/// depends on the baseline mode:
///
/// - image-tree baseline (`--baseline`): a diff gallery deployed at `U`, so
///   `U/baseline` and `U/current`;
/// - manifest baseline (`--baseline-manifest`): no committed baseline PNGs, so a
///   diff gallery is impossible. The current shots are a plain gallery at `U`,
///   and "Before" is left unhosted (no `<img>` that would 404) unless an explicit
///   `--baseline-url` points at a canonical gallery.
fn image_bases(args: &CommentArgs, manifest_mode: bool) -> (Option<String>, Option<String>) {
    let gallery = args.gallery_url.as_deref().map(trim_slash);

    let after = match args.current_url.as_deref() {
        Some(url) => Some(trim_slash(url)),
        None => gallery.as_ref().map(|u| {
            if manifest_mode {
                u.clone()
            } else {
                format!("{u}/current")
            }
        }),
    };

    let before = match args.baseline_url.as_deref() {
        Some(url) => Some(trim_slash(url)),
        // In manifest mode there is no baseline tree to derive from a gallery URL.
        None if manifest_mode => None,
        None => gallery.as_ref().map(|u| format!("{u}/baseline")),
    };

    (before, after)
}

/// Drop a single trailing slash so derived subpaths join cleanly.
fn trim_slash(url: &str) -> String {
    url.trim_end_matches('/').to_owned()
}
