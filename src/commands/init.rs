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
use crate::io::host::{self, HookEnable};

/// Sentinel line in the `.gitignore` block; its presence means the block is
/// already there, keeping a re-run idempotent.
const GITIGNORE_MARKER: &str = "# screencomp: commit the digest baselines";

/// Committed hooks directory wired with `git config core.hooksPath`. A committed
/// path (not `.git/hooks/`, which clones do not carry) makes the guard shareable
/// without depending on a specific hook manager.
const HOOK_PATH: &str = ".githooks/pre-push";

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
    let hook = render_hook();
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

    // Optionally wire the guard so the strict gate's local half actually runs,
    // instead of leaving the developer an undocumented `git config` step (the gap
    // where a scaffolded-but-inert hook silently protects nothing).
    let hook_enable = args
        .enable_hook
        .then(|| host::set_hooks_path(&args.dir, ".githooks"));

    match args.format {
        OutputFormat::Json => write_json(&outcomes, hook_enable.as_ref(), out)?,
        OutputFormat::Human if !ctx.quiet => {
            write_human(&arch, &outcomes, hook_enable.as_ref(), out)?;
        }
        OutputFormat::Human => {}
    }
    Ok(0)
}

/// Render the "enable the guard" line for the human report, reflecting what
/// `--enable-hook` did (or the command to run when it was not used or failed).
fn enable_step(hook_enable: Option<&HookEnable>) -> String {
    match hook_enable {
        Some(HookEnable::Set) => {
            "Enabled the local pre-push guard (core.hooksPath -> .githooks).".to_owned()
        }
        Some(HookEnable::GitUnavailable) => "Could not enable the guard: git is not on PATH. \
             Once installed, run: git config core.hooksPath .githooks"
            .to_owned(),
        Some(HookEnable::Failed(detail)) => format!(
            "Could not enable the guard (git: {detail}). \
             Run it yourself from a git checkout: git config core.hooksPath .githooks"
        ),
        None => "Enable the local pre-push guard (the strict gate's local half):\n   \
             git config core.hooksPath .githooks   (or re-run init with --enable-hook)"
            .to_owned(),
    }
}

/// Human-readable report: one line per file, then next steps.
fn write_human(
    arch: &str,
    outcomes: &[(Utf8PathBuf, Scaffold)],
    hook_enable: Option<&HookEnable>,
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
    // When `--enable-hook` succeeded, the guard is already wired, so step 2 below
    // confirms it; otherwise it prints the command (or why the attempt failed).
    let step_2 = enable_step(hook_enable);
    writeln!(
        out,
        "\nNext steps:\n\
         1. Wire your real capture into .github/workflows/visual-docs.yml and\n   \
         .githooks/pre-push so each writes shots/current/{arch}/captures.json plus\n   \
         the PNGs it references (each shot's name, toggles, hash, and image path).\n   \
         A capture that writes only PNGs can have screencomp author that index:\n   \
         screencomp index --input shots/current --toggles-from-path\n\
         2. {step_2}\n\
         3. Seed the baseline once on {arch} and commit it:\n   \
         screencomp manifest --input shots/current \\\n     \
         --output shots/baseline/{arch}.json\n\
         4. Enable GitHub Pages (Settings -> Pages -> Deploy from a branch: gh-pages /).\n\
         \n\
         To support another CPU arch, add it to [capture].arches in\n\
         screencomp.toml (e.g. arches = [\"{arch}\", \"x86_64\"]); CI gains a lane\n\
         per arch and you seed its baseline the same way.\n\
         \n\
         The gate is strict by default: CI fails on unexpected drift, and you\n\
         regenerate the baseline locally (the pre-push guard) and commit it. To\n\
         switch to CI auto-accept instead, set fail-on-drift:false and\n\
         update-manifest:true in the workflow.\n\
         \n\
         Day-to-day, once set up: change something visual, then `git push`. The\n\
         pre-push guard re-captures and, on drift, regenerates the baseline,\n\
         builds a review gallery, and blocks — review it, `git add` the baseline,\n\
         commit, and push again. That is the whole loop. You never verify your\n\
         CPU arch (it is auto-detected; CI gates its own lane) or pre-flight\n\
         Docker (the guard checks it and fails loudly). It is local-first: you\n\
         own the baseline, so don't wait for CI to handle a visual change."
    )
    .map_err(write_err)
}

/// Stable single-line JSON contract for automation.
fn write_json(
    outcomes: &[(Utf8PathBuf, Scaffold)],
    hook_enable: Option<&HookEnable>,
    out: &mut dyn Write,
) -> Result<(), AppError> {
    #[derive(Serialize)]
    struct File<'a> {
        path: &'a str,
        action: &'a str,
    }
    #[derive(Serialize)]
    struct Report<'a> {
        files: Vec<File<'a>>,
        /// Present only when `--enable-hook` was passed: `enabled`,
        /// `git-unavailable`, or `failed`.
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
    let hook_enabled = hook_enable.map(|outcome| match outcome {
        HookEnable::Set => "enabled",
        HookEnable::GitUnavailable => "git-unavailable",
        HookEnable::Failed(_) => "failed",
    });
    let json = serde_json::to_string(&Report {
        files,
        hook_enabled,
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
# Local pre-push guard (.githooks/pre-push): re-capture only when these globs
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
/// with the pre-push guard and commit). Because the manifest is not auto-pushed,
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

/// The scaffolded pre-push guard. It is the local half of the strict gate:
/// re-capture only when `[guard].paths` change, and block the push on drift so
/// the developer regenerates and commits the baseline before CI ever sees it.
///
/// It detects the host arch at runtime (rather than baking it) so the same
/// committed hook is correct on every developer's machine: ARM and amd64 devs
/// each capture and classify under their own arch. The `scope` relevance check
/// distinguishes "a path matched" (exit 3) from "the check errored" (any other
/// non-zero) — an error skips rather than forcing a slow capture, since CI is the
/// backstop. The capture block is clearly marked for the consumer to adapt.
// llmlint: ignore-block[changed_behavior_has_e2e] This function is the shell template alone — the capture container cannot run in this repository's offline, tempdir-isolated suite; the mapped run is proven end to end before release by the demo journey AGENTS.md requires (demo/ through the pinned image: install, capture, PNG bytes identical to the root-running form, every file under the bind mount owned by the invoking uid), and the four-part contract itself is held here by the checks over all five shipped copies in tests/integration.rs.
fn render_hook() -> String {
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
# The whole loop when you change something visual: edit, then `git push`. This
# hook captures (only if [guard].paths changed) and, on drift, regenerates the
# baseline + builds a review gallery and BLOCKS — then you review the gallery,
# `git add` the baseline, commit, and push again. That is the entire workflow,
# local-first by design: you own the baseline, so don't wait for CI to handle
# drift. Don't pre-check your environment either — your CPU arch is auto-detected
# just below (CI gates its own arch lane; local and CI never need to match) and
# Docker is checked further down and fails loudly if missing. Just run the loop.
#
# Enable once per clone:  git config core.hooksPath .githooks
# Bypass intentionally:   git push --no-verify
set -euo pipefail

# Detect this machine's arch so the same committed hook works for every developer.
# It must be one of [capture].arches in screencomp.toml; if not, classify below
# fails with an explanatory error telling you to add it.
case \"$(uname -m)\" in
  arm64 | aarch64) ARCH=\"arm64\";  DOCKER_PLATFORM=\"linux/arm64\" ;;
  x86_64 | amd64)  ARCH=\"x86_64\"; DOCKER_PLATFORM=\"linux/amd64\" ;;
  *) echo \"pre-push: unsupported arch $(uname -m); bypass with git push --no-verify\" >&2; exit 1 ;;
esac

# ---- adapt these to your repo -----------------------------------------------
MANIFEST=\"shots/baseline/${ARCH}.json\"    # committed baseline for this arch
GALLERY=\"shots/review\"                     # [guard].gallery (review output)
CURRENT=\"shots/current\"                    # capture root, mirrors visual-docs.yml
# -----------------------------------------------------------------------------

# No-op under CI: the visual-docs workflow is the source of truth there.
[ -n \"${CI:-}\" ] && exit 0

# Without the CLI the guard CANNOT evaluate the push, so do NOT skip silently — a
# strict gate you believe is protecting you but isn't is the worst outcome. Warn
# loudly and skip; set SCREENCOMP_GUARD_REQUIRE=1 to fail here instead (safest
# once everyone has the CLI). CI still gates this regardless.
if ! command -v screencomp >/dev/null 2>&1; then
  {
    echo \"pre-push: screencomp is NOT on PATH — the visual guard cannot run.\"
    echo \"          Install it: https://github.com/nickderobertis/screencomp#install\"
    echo \"          then enable the hook: git config core.hooksPath .githooks\"
    echo \"          Set SCREENCOMP_GUARD_REQUIRE=1 to fail here instead.\"
  } >&2
  [ -n \"${SCREENCOMP_GUARD_REQUIRE:-}\" ] && exit 1
  exit 0
fi

# --- 1. Compute the push range(s) from git's stdin ---------------------------
ranges=()
if [ -n \"${SCREENCOMP_GUARD_RANGE:-}\" ]; then
  ranges+=(\"$SCREENCOMP_GUARD_RANGE\")   # test/override: diff this range directly
else
  zero='^0+$'
  while read -r _local_ref local_sha _remote_ref remote_sha; do
    [ -z \"${local_sha:-}\" ] && continue
    if [[ \"$local_sha\" =~ $zero ]]; then
      continue                            # branch deletion: nothing to capture
    elif [[ \"$remote_sha\" =~ $zero ]]; then
      base=\"$(git merge-base origin/HEAD \"$local_sha\" 2>/dev/null || true)\"
      [ -n \"$base\" ] && ranges+=(\"${base}..${local_sha}\") || ranges+=(\"$local_sha\")
    else
      ranges+=(\"${remote_sha}..${local_sha}\")
    fi
  done
fi
[ \"${#ranges[@]}\" -eq 0 ] && exit 0

changed=\"$(for r in \"${ranges[@]}\"; do git diff --name-only \"$r\"; done | sort -u)\"

# --- 2. Cheap path: is anything screenshot-relevant? -------------------------
# `scope` exits 3 when a changed path matches [guard].paths, 0 when none do, and
# any OTHER non-zero on error (an old CLI without `scope`, a bad config, ...).
# Treat ONLY exit 3 as relevant; on an error, warn and skip rather than force a
# slow capture — CI is the backstop, so a false \"relevant\" here is the wrong bet.
set +e
printf '%s\\n' \"$changed\" | screencomp scope --changed-from - --exit-code --quiet
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

# Pass the host's extra CA bundle into the container so a TLS-intercepting egress
# proxy (corporate networks, Codespaces, hosted dev envs) does not break `npm ci`
# with SELF_SIGNED_CERT_IN_CHAIN — which npm hides behind the cryptic `Exit
# handler never called!` until you re-run with --loglevel verbose. No-op when none.
ca_args=()
host_ca=\"${NODE_EXTRA_CA_CERTS:-${SSL_CERT_FILE:-}}\"
if [ -n \"$host_ca\" ] && [ -f \"$host_ca\" ]; then
  ca_args+=(-v \"$host_ca:/host-ca.crt:ro\" \\
    -e NODE_EXTRA_CA_CERTS=/host-ca.crt -e SSL_CERT_FILE=/host-ca.crt)
fi

# ---- adapt this to YOUR stack (same image/flags as visual-docs.yml) ----------
# The container runs as YOU, not root: it bind-mounts this working tree, so a
# root-running capture leaves every file it writes there owned by root and
# unremovable without sudo. Three things have to come with that mapping or the
# container has nowhere to write:
#   * node_modules is masked by a host directory you created (an anonymous
#     `-v /work/node_modules` volume would be created root-owned and `npm ci`
#     could not install into it). It still keeps the install inside the
#     container, matching CI's fresh checkout. Its mountpoint has to exist here
#     too, or Docker creates that one root-owned inside your tree.
#   * HOME points at a host directory you own — your uid has no entry in the
#     image's passwd file, so npm would otherwise have no writable home.
#   * the scratch holding both is removed however this hook exits.
host_user=\"$(id -u):$(id -g)\"
capture_scratch=\"$(mktemp -d)\"
trap 'rm -rf \"$capture_scratch\"' EXIT
mkdir -p \"$capture_scratch/node_modules\" \"$capture_scratch/home\" node_modules
docker run --rm --platform=\"$DOCKER_PLATFORM\" --ipc=host --shm-size=2g \\
  --user \"$host_user\" \\
  -v \"$PWD:/work\" -v \"$capture_scratch:/scratch\" \\
  -v \"$capture_scratch/node_modules:/work/node_modules\" \\
  -e HOME=/scratch/home -w /work \\
  ${ca_args[@]+\"${ca_args[@]}\"} \\
  mcr.microsoft.com/playwright:v1.60.0-noble \\
  bash -lc \"npm ci && SHOTS_OUT=$CURRENT/$ARCH npx playwright test\"
# ------------------------------------------------------------------------------

# --- 4. Classify the capture against the committed baseline manifest ---------
# No --arch needed: screencomp defaults to the host arch from [capture].arches.
set +e
screencomp classify --baseline-manifest \"$MANIFEST\" --current \"$CURRENT\" --exit-code
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
screencomp manifest --input \"$CURRENT\" --output \"$MANIFEST\"
screencomp gallery --input \"$CURRENT\" \\
  --output \"$GALLERY\" --title \"Pre-push screenshot review\" >/dev/null

{
  echo
  echo \"  SCREENSHOTS CHANGED — push blocked for review\"
  echo \"  Review the rendered gallery: $GALLERY/index.html\"
  echo \"  If intended: git add $MANIFEST && git commit and push again.\"
  echo \"  If not: investigate the diff. Bypass with: git push --no-verify\"
  echo
} >&2
exit 1
"
    .to_owned()
}
// llmlint: ignore-end[changed_behavior_has_e2e]
