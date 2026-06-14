//! `screencomp init` — scaffold a visual-docs setup.
//!
//! Day-one wiring is almost entirely boilerplate: a `screencomp.toml`, a CI
//! workflow that captures and then hands the gallery/comment/Pages half to
//! screencomp, and the `.gitignore` lines that commit the tiny digest baselines
//! while ignoring the generated PNGs and galleries. This command writes that
//! scaffold deterministically. It never clobbers existing files unless `--force`
//! is given and reports each path as created, skipped, or overwritten, so it is
//! safe to re-run.

use std::io::Write;

use camino::Utf8PathBuf;
use serde::Serialize;

use super::{Ctx, write_err};
use crate::cli::{InitArgs, OutputFormat};
use crate::errors::AppError;
use crate::io::fs::{self, Scaffold};

/// Sentinel line in the `.gitignore` block; its presence means the block is
/// already there, keeping a re-run idempotent.
const GITIGNORE_MARKER: &str = "# screencomp: commit the digest baselines";

pub(crate) fn run(args: &InitArgs, ctx: &Ctx, out: &mut dyn Write) -> Result<i32, AppError> {
    let toml = render_config(&args.platform);
    let workflow = render_workflow(&args.platform);
    let gitignore = render_gitignore();

    // The config and workflow are plain scaffolds; .gitignore is appended so a
    // pre-existing ignore file is preserved.
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
        OutputFormat::Human if !ctx.quiet => write_human(&args.platform, &outcomes, out)?,
        OutputFormat::Human => {}
    }
    Ok(0)
}

/// Human-readable report: one line per file, then next steps.
fn write_human(
    platform: &str,
    outcomes: &[(Utf8PathBuf, Scaffold)],
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
    writeln!(
        out,
        "\nNext steps:\n\
         1. Wire your capture into .github/workflows/visual-docs.yml so it writes\n   \
         $SHOTS_OUT/<project>/<name>.png.\n\
         2. Seed the baseline once on {platform} and commit it:\n   \
         screencomp manifest --input shots/current --platform {platform} \\\n     \
         --output shots/baseline/{platform}.sha256\n\
         3. Enable GitHub Pages (Settings -> Pages -> GitHub Actions)."
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
    let json = serde_json::to_string(&Report { files })
        .map_err(|e| AppError::io("serializing JSON", std::io::Error::other(e)))?;
    writeln!(out, "{json}").map_err(write_err)
}

/// The scaffolded `screencomp.toml`.
fn render_config(platform: &str) -> String {
    format!(
        "\
# screencomp configuration. See https://github.com/nickderobertis/screencomp.

[comment]
title = \"Visual changes\"
marker = \"screencomp\"        # [A-Za-z0-9_-]; one sticky comment per marker
embed_limit = 10             # embed images inline when <= N shots differ (0 disables)

[guard]
# Optional local pre-push guard: re-capture only when these globs change.
# Drop this section if you do not use the guard (see examples/pre-push).
paths = [\"src/**\", \"**/*.{{css,scss,html}}\"]
platform = \"{platform}\"
manifest = \"shots/baseline/{platform}.sha256\"
gallery = \"shots/review\"
"
    )
}

/// The scaffolded `.gitignore` block. The digest baselines under
/// `shots/baseline/` are intentionally *not* ignored — they are the committed
/// state — while the regenerated captures and galleries are.
fn render_gitignore() -> String {
    format!(
        "\n{GITIGNORE_MARKER} (shots/baseline/*.sha256), not the generated PNGs.\n\
         shots/current/\n\
         shots/verify/\n\
         shots/review/\n\
         /site/\n"
    )
}

/// The scaffolded caller workflow. It invokes screencomp's reusable workflow,
/// pinned to the version of the binary that wrote it, so the downstream half
/// (gate, gallery, Pages, comment) stays in lockstep with this CLI.
fn render_workflow(platform: &str) -> String {
    let version = env!("CARGO_PKG_VERSION");
    format!(
        "\
# Visual docs via screencomp's reusable workflow. The capture step stays yours;
# screencomp owns the classify gate, gallery, GitHub Pages deploy, and the sticky
# before/after PR comment. See the screencomp README (\"GitHub Action\").
name: Visual docs

on:
  pull_request:
  push:
    branches: [main]

permissions:
  contents: write       # commit the regenerated manifest to the PR branch
  pull-requests: write  # post the diff comment
  pages: write          # deploy the gallery
  id-token: write       # Pages deployment auth

jobs:
  visual-docs:
    uses: nickderobertis/screencomp/.github/workflows/visual-docs-reusable.yml@v{version}
    with:
      platform: {platform}
      # Replace with your real capture. It MUST write $SHOTS_OUT/<project>/<name>.png
      # ($SHOTS_OUT is exported as shots/current/{platform}).
      capture-command: |
        npm ci
        npx playwright install --with-deps chromium
        npx playwright test
    secrets:
      # Only needed if the repo enforces required status checks, so the bot's
      # manifest push can trigger workflows (see the README's \"Branch protection\").
      push-token: ${{{{ secrets.VISUAL_DOCS_PUSH_TOKEN }}}}
"
    )
}
