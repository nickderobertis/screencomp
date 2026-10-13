//! `screencomp init` — scaffold a visual-docs setup.
//!
//! Day-one wiring is almost entirely boilerplate: a `screencomp.toml`, a CI
//! workflow that captures and then hands the gallery/comment/Pages half to
//! screencomp and the `.gitignore` lines that commit the
//! tiny digest baselines while ignoring the generated PNGs and galleries. This
//! command writes that scaffold deterministically. It never clobbers existing
//! files unless `--force` is given and reports each path as created, skipped, or
//! overwritten, so it is safe to re-run.
//!
//! The scaffold is the *strict* gate by design: CI fails on unexpected drift and
//! the developer owns the baseline (capture and review it locally,
//! commit it). The path of least resistance is therefore the safe one — flipping
//! to CI auto-accept is a documented opt-in in the generated workflow, not the
//! default.
//!
//! Captures always run in a Linux container, so the only dimension that varies is
//! the CPU arch: `screencomp.toml` declares `[capture].arches`, every command
//! defaults to the host arch, and CI fans out one lane per configured arch.

use std::io::Write;

use camino::Utf8PathBuf;
use serde::Serialize;

use super::{Ctx, arch, write_err};
use crate::cli::{InitArgs, OutputFormat};
use crate::errors::AppError;
use crate::io::fs::{self, Scaffold};

/// Sentinel line in the `.gitignore` block; its presence means the block is
/// already there, keeping a re-run idempotent.
const GITIGNORE_MARKER: &str = "# screencomp: commit the digest baselines";

pub(crate) fn run(args: &InitArgs, ctx: &Ctx, out: &mut dyn Write) -> Result<i32, AppError> {
    // Resolve `auto` to the host arch so the scaffold's `[capture].arches` matches
    // the machine it is generated on (e.g. `arm64` on Apple Silicon). The capture
    // is a Linux container, so only the arch varies.
    let arch = if args.arch == arch::AUTO {
        arch::host_arch()
    } else {
        args.arch.clone()
    };
    let toml = render_config(&arch);
    let workflow = render_workflow();
    let gitignore = render_gitignore();

    // The config and workflow are plain scaffolds;
    // .gitignore is appended so a pre-existing ignore file is preserved.
    let outcomes = [
        (
            args.dir.join("screencomp.toml"),
            fs::write_scaffold(&args.dir.join("screencomp.toml"), &toml, args.force)?,
        ),
        (
            args.dir.join(".github/workflows/visual-docs.yml"),
            fs::write_scaffold(
                &args.dir.join(".github/workflows/visual-docs.yml"),
                &workflow,
                args.force,
            )?,
        ),
        (
            args.dir.join(".gitignore"),
            fs::append_block(&args.dir.join(".gitignore"), &gitignore, GITIGNORE_MARKER)?,
        ),
    ];

    match args.format {
        OutputFormat::Json => write_json(&outcomes, out)?,
        OutputFormat::Human if !ctx.quiet => {
            write_human(&arch, &outcomes, args.enable_hook, out)?;
        }
        OutputFormat::Human => {}
    }
    Ok(0)
}

/// Human-readable report: one line per file, then next steps.
fn write_human(
    arch: &str,
    outcomes: &[(Utf8PathBuf, Scaffold)],
    hook_flag_supplied: bool,
    out: &mut dyn Write,
) -> Result<(), AppError> {
    for (path, outcome) in outcomes {
        let verb = match outcome {
            Scaffold::Created => "created",
            Scaffold::Overwritten => "updated",
            Scaffold::Skipped => "skipped (exists; pass --force to overwrite)",
        };
        writeln!(out, "{verb} {path}").map_err(write_err)?;
    }
    if hook_flag_supplied {
        writeln!(out, "--enable-hook is retained for compatibility: CI handles visual verification, so a duplicate pre-push hook is unnecessary.")
            .map_err(write_err)?;
    }
    writeln!(
        out,
        "\nNext steps:\n\
         1. Wire your capture into .github/workflows/visual-docs.yml so it writes\n   \
         shots/current/{arch}/captures.json and its PNGs. For PNG-only captures:\n   \
         screencomp index --input shots/current --toggles-from-path\n\
         2. Run the same Linux-container capture locally and review changes:\n   \
         screencomp gallery --input shots/current --output shots/gallery\n\
         3. Write the intended baseline and commit it:\n   \
         screencomp manifest --input shots/current --output shots/baseline/{arch}.json\n\
         4. Enable GitHub Pages (Settings -> Pages -> Deploy from a branch: gh-pages /).\n\
         Add CPU arches to [capture].arches and seed each baseline the same way.\n\
         CI fails on unexpected drift. Capture, review the gallery, and commit\n\
         intended baseline changes before pushing."
    )
    .map_err(write_err)
}

/// Stable single-line JSON contract for automation.
fn write_json(outcomes: &[(Utf8PathBuf, Scaffold)], out: &mut dyn Write) -> Result<(), AppError> {
    #[derive(Serialize)]
    struct File<'a> {
        path: &'a str,
        action: &'a str,
    }
    #[derive(Serialize)]
    struct Report<'a> {
        files: Vec<File<'a>>,
        /// Reserved compatibility field; absent because init enables no hook.
        #[serde(skip_serializing_if = "Option::is_none")]
        hook_enabled: Option<&'a str>,
    }

    let files = outcomes
        .iter()
        .map(|(path, outcome)| File {
            path: path.as_str(),
            action: match outcome {
                Scaffold::Created => "created",
                Scaffold::Overwritten => "overwritten",
                Scaffold::Skipped => "skipped",
            },
        })
        .collect();
    let json = serde_json::to_string(&Report {
        files,
        hook_enabled: None,
    })
    .map_err(|e| AppError::io("serializing JSON", std::io::Error::other(e)))?;
    writeln!(out, "{json}").map_err(write_err)
}

/// The scaffolded `screencomp.toml`.
fn render_config(arch: &str) -> String {
    format!(
        "\
# screencomp configuration. See https://github.com/nickderobertis/screencomp.

[capture]
# CPU architectures you maintain screenshots for. Captures run in a Linux
# container, so only the arch varies. Each entry has its own committed baseline
# (shots/baseline/<arch>.json) and gets a CI capture lane; local commands
# default to your host arch and require it to be listed here. Add an arch (e.g.
# \"x86_64\") to support it — note every entry adds a CI job to each run.
arches = [\"{arch}\"]

[comment]
title = \"Visual changes\"
marker = \"screencomp\"        # [A-Za-z0-9_-]; one sticky comment per marker
embed_limit = 10             # embed images inline when <= N shots differ (0 disables)

# Toggle dimensions the gallery renders controls for. A shot's `toggles` in
# captures.json reference these keys, so one screen becomes a single card you
# toggle through (theme, viewport, …) instead of one card per variant. Each
# `values` entry is a value the capture step can produce; the first is the
# gallery default. Uncomment and adapt to the dimensions your capture varies.
# [[toggle]]
# key = \"theme\"
# label = \"Theme\"
# values = [\"light\", \"dark\"]
#
# [[toggle]]
# key = \"viewport\"
# label = \"Viewport\"
# values = [\"desktop\", \"mobile\"]

[guard]
# Optional manual guard (examples/pre-push): re-capture only when these globs
# change. Adjust to the files that actually affect your screenshots.
paths = [\"src/**\", \"**/*.{{css,scss,html}}\"]
gallery = \"shots/review\"
"
    )
}

/// The scaffolded `.gitignore` block. The digest baselines under
/// `shots/baseline/` are intentionally *not* ignored — they are the committed
/// state — while the regenerated captures and galleries are.
fn render_gitignore() -> String {
    format!(
        "\n{GITIGNORE_MARKER} (shots/baseline/*.json), not the generated PNGs.\n\
         shots/current/\n\
         shots/verify/\n\
         shots/review/\n\
         /site/\n"
    )
}

/// The scaffolded caller workflow. It invokes screencomp's reusable workflow,
/// pinned to the version of the binary that wrote it, so the downstream half
/// (gate, gallery, Pages, comment) stays in lockstep with this CLI.
///
/// It carries no arch: the reusable workflow reads `[capture].arches` from the
/// committed `screencomp.toml` and fans out one capture lane per arch on a
/// matching runner, so the arch list has a single source of truth.
///
/// It opts into the strict gate explicitly (`fail-on-drift: true`) so the model
/// is visible in the consumer's own file rather than hidden in a default: CI
/// fails on unexpected drift and the developer owns the baseline (regenerate it
/// manually and commit). Because the manifest is not auto-pushed,
/// no `push-token` secret is needed.
fn render_workflow() -> String {
    let version = env!("CARGO_PKG_VERSION");
    format!(
        "\
# Visual docs via screencomp's reusable workflow. The capture step stays yours;
# screencomp owns the classify gate, gallery, GitHub Pages deploy, and the sticky
# before/after PR comment. See the screencomp README (\"GitHub Action\").
#
# Which CPU arch(es) CI captures is read from [capture].arches in screencomp.toml
# — one capture lane per arch, each on a matching runner. Add an arch there to
# gain a lane.
name: Visual docs

on:
  pull_request:
    # `closed` lets screencomp delete this PR's gh-pages preview once it closes,
    # so stale /pr-<n>/ galleries do not accumulate on the branch.
    types: [opened, synchronize, reopened, closed]
  push:
    branches: [main]
  schedule:
    # Monthly: prune the gh-pages gallery history so the committed PNGs do not grow
    # the branch without bound. Keeps the 20 most recent versions by default (see
    # `gh-pages-history-versions` below). Adjust or drop freely.
    - cron: \"27 4 1 * *\"

permissions:
  contents: write       # push the gh-pages gallery
  pull-requests: write  # post the diff comment
  pages: read           # gate the run on the Pages build the deploy triggers

jobs:
  visual-docs:
    uses: nickderobertis/screencomp/.github/workflows/visual-docs-reusable.yml@v{version}
    with:
      # Strict gate (the safe default): CI FAILS if the capture drifts from the
      # committed baseline. Capture and review locally, then commit the intended
      # baseline. To switch to CI auto-accept
      # instead, set `fail-on-drift: false` and `update-manifest: true` (and wire
      # `push-token` under `secrets:` so the bot's manifest push can trigger CI).
      fail-on-drift: true
      # Keep the gh-pages gallery bounded (the safe default): delete this PR's
      # preview when it closes and prune gh-pages history on the schedule below.
      # The `closed` PR type and the `schedule:` trigger above are what let these
      # run. Set false to opt out (e.g. you serve Pages from somewhere other than
      # the gh-pages branch).
      gh-pages-maintenance: true
      # Most recent gallery versions (gh-pages commits) the scheduled prune keeps
      # intact; older history is collapsed into one base commit. Default 20; set 0
      # to collapse to a single commit.
      gh-pages-history-versions: 20
      # Text-dense screens? Anti-aliased glyph edges can differ in the last bit
      # across heterogeneous CI CPUs (Intel vs AMD on ubuntu-latest), flipping a
      # dense-text shot between otherwise-identical re-runs. Capturing at
      # deviceScaleFactor >= 2 (set in your Playwright config) spreads each
      # sub-pixel shift across more anti-aliasing gradations, so most device pixels
      # stay under the 1/255 quantization step instead of flipping — the usual fix.
      # It is ~4x the bytes, so apply it to text-dense lanes, not pure-graphical
      # UIs. See the screencomp README \"Cross-CPU\" troubleshooting.
      # Replace with your real capture. It MUST leave $SHOTS_OUT/captures.json
      # (each shot's name, toggles, hash, and image path) beside the PNGs it
      # references ($SHOTS_OUT is exported as shots/current/<arch> for each lane).
      # The capture runs in the container with no host tools, so to have the CLI
      # author that index instead, install screencomp in the container and end with:
      #   screencomp index --input \"$SHOTS_OUT\" --toggles-from-path
      capture-command: |
        npm ci
        npx playwright install --with-deps chromium
        npx playwright test
"
    )
}
