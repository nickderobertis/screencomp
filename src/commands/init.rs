//! `screencomp init` — scaffold a visual-docs setup.
//!
//! Day-one wiring is almost entirely boilerplate: a `screencomp.toml`, a CI
//! workflow that captures and then hands the gallery/comment/Pages half to
//! screencomp, a local pre-push guard, and the `.gitignore` lines that commit the
//! tiny digest baselines while ignoring the generated PNGs and galleries. This
//! command writes that scaffold deterministically. It never clobbers existing
//! files unless `--force` is given and reports each path as created, skipped, or
//! overwritten, so it is safe to re-run.
//!
//! The scaffold is the *strict* gate by design: CI fails on unexpected drift and
//! the developer owns the baseline (regenerate it locally via the pre-push guard,
//! commit it). The path of least resistance is therefore the safe one — flipping
//! to CI auto-accept is a documented opt-in in the generated workflow, not the
//! default.

use std::io::Write;

use camino::Utf8PathBuf;
use serde::Serialize;

use super::{Ctx, platform, write_err};
use crate::cli::{InitArgs, OutputFormat};
use crate::errors::AppError;
use crate::io::fs::{self, Scaffold};

/// Sentinel line in the `.gitignore` block; its presence means the block is
/// already there, keeping a re-run idempotent.
const GITIGNORE_MARKER: &str = "# screencomp: commit the digest baselines";

/// Committed hooks directory wired with `git config core.hooksPath`. A committed
/// path (not `.git/hooks/`, which clones do not carry) makes the guard shareable
/// without depending on a specific hook manager.
const HOOK_PATH: &str = ".githooks/pre-push";

pub(crate) fn run(args: &InitArgs, ctx: &Ctx, out: &mut dyn Write) -> Result<i32, AppError> {
    // Resolve `auto` to the host `<os>-<arch>` so the scaffold matches the
    // machine it is generated on, consistent with `--platform auto` elsewhere.
    let platform = platform::resolve(&args.platform);
    let toml = render_config(&platform);
    let workflow = render_workflow(&platform);
    let hook = render_hook(&platform);
    let gitignore = render_gitignore();

    // The config and workflow are plain scaffolds; the hook is executable;
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
            args.dir.join(HOOK_PATH),
            fs::write_executable_scaffold(&args.dir.join(HOOK_PATH), &hook, args.force)?,
        ),
        (
            args.dir.join(".gitignore"),
            fs::append_block(&args.dir.join(".gitignore"), &gitignore, GITIGNORE_MARKER)?,
        ),
    ];

    match args.format {
        OutputFormat::Json => write_json(&outcomes, out)?,
        OutputFormat::Human if !ctx.quiet => write_human(&platform, &outcomes, out)?,
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
         1. Wire your real capture into .github/workflows/visual-docs.yml and\n   \
         .githooks/pre-push so each writes shots/current/{platform}/<project>/<name>.png.\n\
         2. Enable the local pre-push guard (the strict gate's local half):\n   \
         git config core.hooksPath .githooks\n\
         3. Seed the baseline once on {platform} and commit it:\n   \
         screencomp manifest --input shots/current --platform {platform} \\\n     \
         --output shots/baseline/{platform}.sha256\n\
         4. Enable GitHub Pages (Settings -> Pages -> Deploy from a branch: gh-pages /).\n\
         \n\
         The gate is strict by default: CI fails on unexpected drift, and you\n\
         regenerate the baseline locally (the pre-push guard) and commit it. To\n\
         switch to CI auto-accept instead, set fail-on-drift:false and\n\
         update-manifest:true in the workflow."
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
# Local pre-push guard (.githooks/pre-push): re-capture only when these globs
# change. Adjust to the files that actually affect your screenshots.
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
///
/// It opts into the strict gate explicitly (`fail-on-drift: true`) so the model
/// is visible in the consumer's own file rather than hidden in a default: CI
/// fails on unexpected drift and the developer owns the baseline (regenerate it
/// with the pre-push guard and commit). Because the manifest is not auto-pushed,
/// no `push-token` secret is needed.
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

jobs:
  visual-docs:
    uses: nickderobertis/screencomp/.github/workflows/visual-docs-reusable.yml@v{version}
    with:
      platform: {platform}
      # Strict gate (the safe default): CI FAILS if the capture drifts from the
      # committed baseline. Regenerate the baseline locally with the pre-push
      # guard (.githooks/pre-push) and commit it. To switch to CI auto-accept
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
      # Replace with your real capture. It MUST write $SHOTS_OUT/<project>/<name>.png
      # ($SHOTS_OUT is exported as shots/current/{platform}).
      capture-command: |
        npm ci
        npx playwright install --with-deps chromium
        npx playwright test
"
    )
}

/// Map a `<os>-<arch>` platform key to the `docker run --platform` value the hook
/// captures under. Anything aarch64/arm64 is `linux/arm64`; everything else
/// defaults to `linux/amd64` (the standard pinned-container target).
fn docker_platform(platform: &str) -> &'static str {
    if platform.contains("arm64") || platform.contains("aarch64") {
        "linux/arm64"
    } else {
        "linux/amd64"
    }
}

/// The scaffolded pre-push guard. It is the local half of the strict gate:
/// re-capture only when `[guard].paths` change, and block the push on drift so
/// the developer regenerates and commits the baseline before CI ever sees it.
///
/// The platform and its `docker --platform` are baked in for the chosen key, and
/// the `scope` relevance check distinguishes "a path matched" (exit 3) from "the
/// check errored" (any other non-zero) — an error skips rather than forcing a
/// slow capture, since CI is the backstop. The capture block is clearly marked
/// for the consumer to adapt to their stack.
fn render_hook(platform: &str) -> String {
    let docker_platform = docker_platform(platform);
    format!(
        "\
#!/usr/bin/env bash
# screencomp local pre-push guard (scaffolded by `screencomp init`).
#
# Local half of the STRICT gate: CI fails on unexpected drift, and this hook lets
# you regenerate and commit the baseline BEFORE pushing so CI stays green. It
# re-captures only when screenshot-relevant files change ([guard].paths in
# screencomp.toml) and blocks the push on drift. Capture and git stay in your
# hands; the marked capture block below is yours to adapt.
#
# Enable once per clone:  git config core.hooksPath .githooks
# Bypass intentionally:   git push --no-verify
set -euo pipefail

# ---- adapt these to your repo; mirror screencomp.toml's [guard] --------------
PLATFORM=\"{platform}\"                          # [guard].platform
DOCKER_PLATFORM=\"{docker_platform}\"               # capture container arch for $PLATFORM
MANIFEST=\"shots/baseline/${{PLATFORM}}.sha256\"  # [guard].manifest (committed)
GALLERY=\"shots/review\"                         # [guard].gallery  (review output)
CURRENT=\"shots/current\"                        # capture root, mirrors visual-docs.yml
CONFIG=\"${{SCREENCOMP_CONFIG:-screencomp.toml}}\" # supplies [guard].paths to `scope`
# ------------------------------------------------------------------------------

# No-op under CI: the visual-docs workflow is the source of truth there.
[ -n \"${{CI:-}}\" ] && exit 0

# If the CLI is not installed, do not block the push — just say so.
if ! command -v screencomp >/dev/null 2>&1; then
  echo \"pre-push: screencomp not on PATH; skipping the visual guard\" >&2
  exit 0
fi

# --- 1. Compute the push range(s) from git's stdin ---------------------------
ranges=()
if [ -n \"${{SCREENCOMP_GUARD_RANGE:-}}\" ]; then
  ranges+=(\"$SCREENCOMP_GUARD_RANGE\")   # test/override: diff this range directly
else
  zero='^0+$'
  while read -r _local_ref local_sha _remote_ref remote_sha; do
    [ -z \"${{local_sha:-}}\" ] && continue
    if [[ \"$local_sha\" =~ $zero ]]; then
      continue                            # branch deletion: nothing to capture
    elif [[ \"$remote_sha\" =~ $zero ]]; then
      base=\"$(git merge-base origin/HEAD \"$local_sha\" 2>/dev/null || true)\"
      [ -n \"$base\" ] && ranges+=(\"${{base}}..${{local_sha}}\") || ranges+=(\"$local_sha\")
    else
      ranges+=(\"${{remote_sha}}..${{local_sha}}\")
    fi
  done
fi
[ \"${{#ranges[@]}}\" -eq 0 ] && exit 0

changed=\"$(for r in \"${{ranges[@]}}\"; do git diff --name-only \"$r\"; done | sort -u)\"

# --- 2. Cheap path: is anything screenshot-relevant? -------------------------
# `scope` exits 3 when a changed path matches [guard].paths, 0 when none do, and
# any OTHER non-zero on error (an old CLI without `scope`, a bad config, ...).
# Treat ONLY exit 3 as relevant; on an error, warn and skip rather than force a
# slow capture — CI is the backstop, so a false \"relevant\" here is the wrong bet.
set +e
printf '%s\\n' \"$changed\" | screencomp scope --config \"$CONFIG\" --changed-from - --exit-code --quiet
scope_status=$?
set -e
case \"$scope_status\" in
  0) exit 0 ;;                            # nothing relevant: pass silently
  3) : ;;                                 # a relevant path matched: capture below
  *)
    echo \"pre-push: 'screencomp scope' failed (exit $scope_status); skipping the\" >&2
    echo \"          visual guard. Is screencomp current? CI still gates this.\" >&2
    exit 0
    ;;
esac

# --- 3. Relevant change: capture requires Docker -----------------------------
if ! command -v docker >/dev/null 2>&1 || ! docker info >/dev/null 2>&1; then
  echo \"pre-push: screenshot-relevant files changed but Docker is unavailable to\" >&2
  echo \"          re-capture. Refusing to push (a pass would be false assurance).\" >&2
  echo \"          Start Docker and retry, or bypass with: git push --no-verify\" >&2
  exit 1
fi

# ---- adapt this to YOUR stack (same image/flags as visual-docs.yml) ----------
docker run --rm --platform=\"$DOCKER_PLATFORM\" --ipc=host --shm-size=2g \\
  -v \"$PWD:/work\" -w /work \\
  mcr.microsoft.com/playwright:v1.60.0-noble \\
  bash -lc \"npm ci && SHOTS_OUT=$CURRENT/$PLATFORM npx playwright test\"
# ------------------------------------------------------------------------------

# --- 4. Classify the capture against the committed baseline manifest ---------
set +e
screencomp classify --baseline-manifest \"$MANIFEST\" --current \"$CURRENT\" \\
  --platform \"$PLATFORM\" --exit-code
status=$?
set -e

if [ \"$status\" -eq 0 ]; then
  echo \"pre-push: screenshots unchanged against $MANIFEST — ok to push\"
  exit 0
elif [ \"$status\" -ne 3 ]; then
  echo \"pre-push: screencomp classify failed (exit $status)\" >&2
  exit \"$status\"
fi

# --- On drift (classify exit 3): regenerate, build a gallery, BLOCK. ---------
screencomp manifest --input \"$CURRENT\" --platform \"$PLATFORM\" --output \"$MANIFEST\"
screencomp gallery --input \"$CURRENT\" --platform \"$PLATFORM\" \\
  --output \"$GALLERY\" --title \"Pre-push screenshot review\" >/dev/null

{{
  echo
  echo \"  SCREENSHOTS CHANGED — push blocked for review\"
  echo \"  Review the rendered gallery: $GALLERY/index.html\"
  echo \"  If intended: git add $MANIFEST && git commit and push again.\"
  echo \"  If not: investigate the diff. Bypass with: git push --no-verify\"
  echo
}} >&2
exit 1
"
    )
}
