# screencomp

Deterministic screenshot tooling for the **visual-docs framework**: classify a
capture against a baseline, render a static HTML gallery, and produce the sticky
pull-request comment — all byte-reproducible and network-free.

`screencomp` is the *publish CLI* a reusable visual-regression workflow calls
after it captures screenshots. Because captures are byte-reproducible, baselines
can be recomputed rather than committed, so repositories avoid binary churn.

## What it does

Screenshots follow a single convention — `<root>/<project>/<name>.png` — where
each `<project>` is a Playwright project/variant. From two such trees,
`screencomp`:

- **`classify`** — compares `current` against `baseline` and labels each
  screenshot `added` / `changed` / `removed` / `unchanged` (by content hash).
- **`gallery`** — renders a self-contained `index.html` index of a tree, or a
  before/after **diff gallery** when given a `--baseline` (great for PR previews).
- **`comment`** — renders the sticky Markdown PR comment body for a
  classification (with a stable HTML marker for upserts).
- **`manifest`** — writes a tree's digests as a tiny text file usable as a
  committed, image-free baseline (`classify`/`comment` accept it via
  `--baseline-manifest`).
- **`verify`** — asserts two captures of the *same* build are byte-identical
  (the [reproducibility gate](#reproducibility-gate-verify)); exits non-zero the
  moment they diverge.
- **`doctor`** — a [preflight](#preflight-doctor) that prints the resolved
  platform key and sanity-checks the `<project>/<name>.png` layout before you
  classify.

It never decodes images — it content-hashes bytes — so output is deterministic
and the tool has no image-codec dependencies.

Because that content hash also changes with the renderer's OS and CPU
architecture, each command takes an optional `--platform` to compare within a
single `<root>/<platform>/<project>/<name>.png` subtree (see
[Cross-platform comparison](#cross-platform-comparison)).

## Install

### Install script (recommended)

Download the prebuilt binary for your platform, verify its SHA-256 checksum, and
install it to `~/.local/bin`:

```sh
curl -fsSL https://raw.githubusercontent.com/nickderobertis/screencomp/main/scripts/install.sh | sh
```

Pin a version or choose where it lands (flags or the matching environment
variables `SCREENCOMP_VERSION` / `SCREENCOMP_INSTALL_DIR`):

```sh
curl -fsSL https://raw.githubusercontent.com/nickderobertis/screencomp/main/scripts/install.sh \
  | sh -s -- --version v0.1.0 --to /usr/local/bin
```

It covers Linux and macOS (x86_64, arm64) and Windows x86_64 under a POSIX shell
(Git Bash / MSYS / WSL), and aborts rather than install a binary it cannot
checksum-verify. Set `GITHUB_TOKEN` if the GitHub API rate-limits the `latest`
lookup.

### From release binaries

Prefer to do it by hand? Prebuilt archives (with SHA-256 checksums) are attached
to each [GitHub Release](https://github.com/nickderobertis/screencomp/releases)
for Linux (x86_64, arm64), macOS (x86_64, arm64), and Windows (x86_64). Download
the archive for your platform, verify the checksum, extract, and place
`screencomp` on your `PATH`.

### With cargo (from git)

For Rust users, or targets without a prebuilt binary, build and install from the
repository (the crate is not yet published to crates.io):

```sh
cargo install --git https://github.com/nickderobertis/screencomp --locked screencomp
```

> Once crates.io publishing is enabled (see [Releasing](#releasing)),
> `cargo install screencomp --locked` will also work.

### From source

```sh
git clone https://github.com/nickderobertis/screencomp
cd screencomp
just build-release   # binary at target/release/screencomp
```

### As a GitHub Action

#### Pick your gate

Decide how CI should react to a visual change before wiring the workflow — this is
the one choice that shapes everything downstream:

- **Strict gate + local pre-push guard (recommended).** CI **fails** when a
  capture drifts from the committed baseline. You regenerate and commit the
  baseline locally — the [pre-push guard](#local-pre-push-guard-the-strict-gates-local-half)
  re-captures only when screenshot-relevant files change and blocks the push until
  you do — so an intended change is already committed by the time CI runs: green on
  intended changes, red only on drift you missed. Pushes nothing back, so it needs
  no elevated token and just works under branch protection. This is what
  [`screencomp init`](#scaffold-a-setup-init) scaffolds.
- **CI auto-accept (lighter).** CI regenerates the baseline and pushes it to the
  PR branch; drift never turns the check red. Less to set up, but an unintended
  visual change can slip through unnoticed, and the manifest push needs a
  trigger-capable token under branch protection.

The reusable workflow and composite action default to the strict gate
(`fail-on-drift: true`, `update-manifest: false`). For auto-accept, set
`fail-on-drift: false` and `update-manifest: true`.

#### Installing the CLI

A composite action installs the CLI (and optionally runs it) on a runner:

```yaml
- uses: nickderobertis/screencomp@v1
  with:
    version: latest        # or a pinned tag like v0.1.0
- run: screencomp classify --baseline shots/baseline --current shots/current
```

Inputs: `version` (release tag or `latest`), `method` (`download` prebuilt asset,
the default, or `source` to build with `cargo install` from the checkout or git),
and `args` (run `screencomp <args>` right after install). Downloads are verified
against their published SHA-256.

#### Batteries-included reusable workflow

The PR-review half — gate, gallery, GitHub Pages deploy, sticky comment — is the
same for everyone, so screencomp ships it as a reusable workflow. You own only
the capture; it owns everything downstream:

```yaml
jobs:
  visual-docs:
    uses: nickderobertis/screencomp/.github/workflows/visual-docs-reusable.yml@v1
    with:
      platform: linux-x86_64
      capture-command: |        # MUST write $SHOTS_OUT/<project>/<name>.png
        npm ci
        npx playwright install --with-deps chromium
        npx playwright test
    secrets:
      push-token: ${{ secrets.VISUAL_DOCS_PUSH_TOKEN }}   # optional; see below
```

It runs the reproducibility gate, classifies against the committed digest
manifest (failing the job on drift under the default strict gate), builds the
gallery, deploys a canonical gallery from the default branch and a per-PR
`/pr-<n>/` preview on pull requests, waits for Pages to go live, and posts a
sticky before/after comment that sources "After" from the PR preview and "Before"
from the canonical main gallery — and diffs against the PR base branch's manifest
so the intended change still shows even when the committed baseline already
matches. `screencomp init` scaffolds a caller for it
([`init`](#scaffold-a-setup-init)). Key gate inputs: `fail-on-drift` (default
`true`; set `false` to auto-accept), `update-manifest` (default `false`; set
`true` to have CI push the regenerated baseline), and `comment-base-ref` (the ref
the comment's "Before" comes from; defaults to the PR base branch).

> [!IMPORTANT]
> **Inline thumbnails need a *public* Pages site.** GitHub renders comment images
> through its anonymous Camo proxy, which can only fetch publicly reachable URLs.
> A private Pages gallery still works as a *link*, but its inline before/after
> previews will not load — keep the gallery public if you want them. If the repo
> enforces required status checks, also set `VISUAL_DOCS_PUSH_TOKEN` to a
> credential that can trigger workflows (a fine-grained PAT or App token), because
> the default `GITHUB_TOKEN`'s manifest push starts no runs and strands the PR.

**Keeping the `gh-pages` branch bounded.** The source repo never accumulates
binary history — it commits only the [digest manifest](#image-free-baselines-digest-manifest),
never PNGs. But the *galleries* do commit PNGs to the `gh-pages` branch, so left
alone that branch grows without bound: per-PR `/pr-<n>/` previews are never
removed (a `main` deploy must not clobber live previews, so it can't prune them
either), and every changed shot leaves its old blob in history forever. The
reusable workflow caps both **by default** (`gh-pages-maintenance: true`) with
two jobs; the `init` scaffold forwards the triggers they need, so it just works:

- `pull_request: closed` runs a **cleanup** job that deletes that PR's
  `/pr-<n>/` preview from `gh-pages`, so closed PRs stop piling up.
- a `schedule:` (cron) trigger runs a **prune** job that squashes `gh-pages` to a
  single fresh commit holding the current site, discarding the accreted blob
  history. It's a destructive rewrite of the *generated* branch only — nothing
  bases work on it, and the canonical gallery is rebuilt on the next default-branch
  push. Schedule it at a quiet hour.

Both are gated on `pages` + `publish`, so a dry run or a Pages-less setup skips
them. Opt out with `gh-pages-maintenance: false` — no need to touch your
triggers. A reusable workflow can't add its own `schedule:`/`pull_request: closed`
triggers, so a hand-rolled caller must still forward those two for the jobs to
fire (then they run by default, like the scaffold). The
[`actions/deploy-pages`](examples/visual-docs.yml) example below uses the GitHub
Actions Pages source instead of a branch, so it has no `gh-pages` history to
prune — at the cost of per-PR previews, which that source can't host.

Two copy-paste templates: [`examples/visual-docs-custom.yml`](examples/visual-docs-custom.yml)
is the **thin** one — the reusable workflow written out as plain jobs over
screencomp's composable actions, so you own only the capture steps;
[`examples/visual-docs.yml`](examples/visual-docs.yml) is the **raw-CLI** one that
calls the binary step by step and deploys via the Actions Pages source.

#### When your capture needs custom steps: the composite actions

A reusable workflow takes a `capture-command` *string*, so it can't host capture
steps that must be GitHub Actions — private-registry OIDC auth, `aws-actions/*`,
a vendored setup action. The reusable workflow is itself just thin glue over
**composable actions** — `screencomp` (install), `screencomp/visual-docs` (the
gate/gallery/Pages/comment half), and `screencomp/gh-pages-maintenance` (preview
cleanup + history prune) — so for custom capture you write the same jobs yourself
and inject whatever you need. "Add OIDC auth", "swap the registry", or "install an
extra package" is just another step you control, no framework to reimplement.
The full worked version (AWS CodeArtifact via OIDC, plus the gh-pages upkeep jobs)
is [`examples/visual-docs-custom.yml`](examples/visual-docs-custom.yml); the core
is just the report half:

```yaml
jobs:
  visual-docs:
    runs-on: ubuntu-latest
    permissions: { contents: write, pull-requests: write }
    steps:
      - uses: actions/checkout@v4
      - uses: your-org/codeartifact-npm-auth@v1          # inject any steps you need
      - uses: your-org/install-aws-cli@v1                #   ↑
      - run: npm ci && npx playwright install --with-deps chromium && npx playwright test
      - uses: nickderobertis/screencomp@v1               # install the CLI
      - uses: nickderobertis/screencomp/visual-docs@v1   # the report half, one step
        with:
          platform: linux-x86_64   # or "" for a project-level layout (no platform subtree)
          fail-on-drift: true      # strict gate (default): fail on unexpected drift
          pages: true
          github-token: ${{ github.token }}
```

The action expects the capture already on disk (`current`, default `shots/current`)
and the CLI installed; it runs the gate, classify, gallery, Pages deploy, and PR
comment. It needs host tools (`gh`, `git`) **and a real git checkout**, so run it
in a host job that consumes the capture as an artifact — never inside your capture
container, whose checkout often lacks `.git` (which breaks the manifest push and
the comment's base-ref diff). Key inputs: `platform` (empty = project-level),
`fail-on-drift` (default `true`, the strict gate; `false` to auto-accept),
`update-manifest` (default `false`; `true` to push the regenerated baseline),
`comment-base-ref` (the "Before" ref; defaults to the PR base branch), `pages`,
`publish` (false for a side-effect-free dry run), `verify-second` (a second
capture dir to assert byte-identical).

### Container image

Multi-arch images are published to GitHub Container Registry. The entrypoint is
`screencomp`; pass `--user` so files written to a bind mount stay owned by you:

```sh
docker run --rm --user "$(id -u):$(id -g)" -v "$PWD:/work" \
  ghcr.io/nickderobertis/screencomp:latest \
  gallery --input shots/current --output site --title "UI"
```

## Usage

```sh
screencomp --help
screencomp classify --baseline path/to/baseline --current path/to/current
```

### Scaffold a setup (`init`)

New to screencomp? Scaffold the day-one boilerplate, then fill in your capture:

```sh
screencomp init --platform linux-x86_64
```

This scaffolds the [strict gate](#pick-your-gate) turnkey — the safe path is the
one-command one:

- `screencomp.toml` — config, including the `[guard]` globs the pre-push hook uses.
- `.github/workflows/visual-docs.yml` — a caller for the
  [reusable workflow](#batteries-included-reusable-workflow) with `fail-on-drift:
  true`, so CI fails on unexpected drift.
- `.githooks/pre-push` — the local guard, executable and with your platform baked
  in; enable it once per clone with `git config core.hooksPath .githooks`.
- the `.gitignore` lines that commit the tiny digest baselines while ignoring
  generated PNGs and galleries.

It never overwrites existing files (pass `--force` to), and appends the
`.gitignore` block idempotently, so it is safe to re-run. After wiring your
capture into both the workflow and the hook, seed the baseline once and commit it
(the `init` output prints the exact command). The gate is strict by default; to
switch to [CI auto-accept](#pick-your-gate) set `fail-on-drift: false` and
`update-manifest: true` in the workflow.

Given trees laid out as `<root>/<project>/<name>.png`:

```text
baseline/desktop/home.png      current/desktop/home.png      # unchanged
baseline/desktop/about.png     current/desktop/about.png     # changed
                               current/desktop/pricing.png   # added
baseline/mobile/home.png                                     # removed
```

Classify (human, then machine-readable):

```sh
$ screencomp classify --baseline baseline --current current
changed desktop/about
added desktop/pricing
removed mobile/home
added 1 changed 1 removed 1 unchanged 1

$ screencomp classify --baseline baseline --current current --format json
{"entries":[…],"counts":{"added":1,"changed":1,"removed":1,"unchanged":1},"changed":true}
```

Build a gallery and render the PR comment:

```sh
# Latest gallery (one tree) — e.g. published from the default branch.
screencomp gallery --input current --output public/screenshots --title "UI"

# Before/after diff gallery (current vs baseline) — e.g. a per-PR preview.
screencomp gallery --input current --baseline baseline \
    --output public/pr-123 --title "PR #123 visual diff"

screencomp comment --baseline baseline --current current \
    --gallery-url https://example.github.io/repo/pr-123/ \
    --output comment.md
```

The diff gallery groups shots into Changed (rendered before/after), Added,
Removed, and Unchanged, and copies both image trees so it is self-contained at
`<output>/baseline/<project>/<name>.png` and `<output>/current/<project>/<name>.png`.
A plain gallery (no `--baseline`) instead lays a single tree out flat at
`<output>/<project>/<name>.png`.

When the diff is small (at most `comment.embed_limit` screenshots differ — 10 by
default) and the comment can resolve an image URL, it embeds the changed shots
inline (changed before/after, added/removed as a single image) and still links to
the full gallery. Larger diffs fall back to a path listing plus the link.
Override the threshold with `--embed-limit <N>` (`0` disables embedding).

The comment resolves its "Before" and "After" image URLs to match the gallery
layout above:

- `--gallery-url <URL>` is the "View full gallery" link and, on its own, derives
  the preview bases from what `gallery` writes. With an image-tree baseline
  (`--baseline`) that is a diff gallery, so `<URL>/baseline/…` and `<URL>/current/…`.
  With `--baseline-manifest` no baseline PNGs exist, so it points "After" at a
  plain gallery of the current shots (`<URL>/…`) and omits "Before" rather than
  emit a baseline URL that would 404.
- `--baseline-url <URL>` / `--current-url <URL>` override either side explicitly,
  each in the plain `<URL>/<project>/<name>.png` layout. This is how manifest mode
  still shows a real before/after diff: point `--baseline-url` at a canonical/main
  gallery and `--current-url` at the per-PR one.

`classify --exit-code` returns a non-zero status when differences exist, for
automation that wants a signal without parsing output:

```sh
screencomp classify --baseline baseline --current current --exit-code || echo "changed"
```

`--quiet` suppresses human output (machine-readable `--format json` is
unaffected).

### Image-free baselines (digest manifest)

Since comparison is by content digest, the baseline pixels are unnecessary — only
the per-shot digests are. `screencomp manifest` writes them as a tiny
`sha256sum`-style text file, which you commit *instead of* the PNGs so the
repository never accumulates binary history:

```sh
# Record the current capture as the baseline (one line per shot).
screencomp manifest --input shots/current --platform auto \
    --output shots/baseline/linux-x86_64.sha256

# Later, classify a new capture against that manifest — no baseline images.
screencomp classify --baseline-manifest shots/baseline/linux-x86_64.sha256 \
    --current shots/current --platform auto
```

`--baseline-manifest` is accepted by `classify` and `comment` as a drop-in
alternative to `--baseline <DIR>` (exactly one is required). The manifest is
already platform-specific, so `--platform` then scopes only `--current`. Its
diff in a pull request (old hash → new hash per shot) is an exact, reviewable
record of what changed; render the actual pixels with `gallery` (which still
needs an image tree). See [`examples/visual-docs.yml`](examples/visual-docs.yml).

### Cross-platform comparison

Identical UI rendered on a different OS or CPU architecture produces
byte-different PNGs, so comparing across platforms reports spurious changes.
Give each capture environment its own subtree and pass `--platform` to compare
only within it:

```text
shots/baseline/linux-x86_64/desktop/home.png
shots/baseline/macos-arm64/desktop/home.png
shots/current/linux-x86_64/desktop/home.png
shots/current/macos-arm64/desktop/home.png
```

```sh
# Explicit key (e.g. one matrix leg per platform in CI):
screencomp classify --baseline shots/baseline --current shots/current \
    --platform linux-x86_64

# `auto` detects the host's own <os>-<arch>, ideal for a local pre-push check:
screencomp classify --baseline shots/baseline --current shots/current \
    --platform auto
```

`--platform` accepts any subtree name; `auto` resolves to `<os>-<arch>` for the
running binary (`aarch64` is spelled `arm64`). All three commands accept it. For
`comment`, give each platform a distinct `--marker` (and optionally `--title`)
so every platform keeps its own sticky comment:

```sh
screencomp comment --baseline shots/baseline --current shots/current \
    --platform linux-x86_64 \
    --marker screencomp-linux-x86_64 --title "Visual changes (linux-x86_64)"
```

Omit `--platform` entirely to treat the root as project-level (no platform
layer) — the original behavior.

Because the comparison is a byte digest, determinism is a *capture-time*
concern: a screenshot's bytes depend on the renderer's OS, CPU, fonts, and GPU.
The recommended standard is to capture **and** compare inside a single pinned
`linux/amd64` container everywhere — including on macOS, where Docker runs a
Linux VM, so `--platform=linux/amd64` reproduces the same `linux-x86_64` pixels
as CI. That gives one platform key and byte-for-byte reproducibility (the key
flag is `--disable-skia-runtime-opts`, which forces a CPU-independent render
path). Run screencomp inside the container so `--platform auto` resolves to
`linux-x86_64`. See [`examples/visual-docs.yml`](examples/visual-docs.yml) for
the full standard configuration, the deterministic-rendering flags, and a
reproducibility gate. Capturing on multiple native platforms instead (e.g. a
real `macos-arm64` lane) is supported by the same `--platform` mechanism, at the
cost of giving up byte-exactness across them.

For a complete, continuously tested consumer of this standard, see
[**screencomp-demo**](https://github.com/nickderobertis/screencomp-demo): it
captures real Playwright screenshots in the pinned container and exercises the
manifest → classify → gallery → comment flow on every pull request, so you can
watch the whole setup work before adopting it.

On Apple Silicon, prefer Docker Desktop's **Rosetta** for amd64 emulation; under
the QEMU fallback the `--use-angle=swiftshader` path can crash Chromium
(`qemu: uncaught target signal`). CPU rasterization (`--disable-gpu`, one browser
per context) with `--disable-skia-runtime-opts` still produces captures
byte-identical to native CI — `screencomp-demo` verifies emulated-vs-native on
every run.

### Reproducibility gate (`verify`)

Image-free baselines are only safe if capture is *deterministic*: a committed
digest means nothing if the same build hashes differently on the next run. Make
that a hard, mechanical check — capture the same build **twice** and require the
two trees to be byte-identical:

```sh
# Capture once to shots/run-a, again to shots/run-b (same build, same flags).
screencomp verify --first shots/run-a --second shots/run-b --platform auto
```

`verify` exits `0` when every shot matches and `3` the moment any diverges,
listing each divergent shot as `differs` (bytes changed between runs),
`only-in-first`, or `only-in-second` (the capture *set* was nondeterministic).
`--format json` emits a single-line `{"reproducible":…,"checked":…,"divergent":[…]}`
contract; `--quiet` suppresses the human report but still gates.

This is the step that turns "it looks fine" into a deterministic, fixable
failure, so run it on every pull request (or nightly to halve capture cost). A
failure is almost always a JS-animated or async-rendered widget caught
mid-transition — see [Capturing an interactive app](#capturing-an-interactive-app)
for the fix.

### Preflight (`doctor`)

Before the first classify, confirm captures actually landed where every command
looks for them. `doctor` resolves the platform key and scans the
`<root>/<project>/<name>.png` layout:

```sh
$ screencomp doctor --input shots/current --platform auto
platform: linux-x86_64 (auto)
inspected: shots/current/linux-x86_64
projects: 2
  desktop (3 shots)
  mobile (1 shot)
shots: 4
ok: layout matches <project>/<name>.png
```

It flags the two mistakes that otherwise surface only as a confusing *empty
diff*: a capture written to the wrong path (`.png` files stranded at the root
instead of under a `<project>/` directory), and an empty capture — often a
`--platform` key that does not match the subtree on disk. Pass `--exit-code` to
turn either problem into a non-zero (`3`) status for a CI preflight gate, or
`--format json` for a machine-readable report.

Pass `--baseline-manifest <file>` to also sanity-check a committed manifest
against the capture. `doctor` then warns on the *other* confusing failure — when
**every** shot looks changed — which is almost always a baseline captured on a
different OS/arch than the host, not a real diff. It catches it two ways: a
`<platform>.sha256` filename naming a platform other than the capture's, and
shared shots with zero unchanged. Both are advisory and never fail the gate.

## Capturing an interactive app

The [`screencomp-demo`](https://github.com/nickderobertis/screencomp-demo)
standard captures static `.html` over `file://` with no interaction. A real
single-page app — client-side navigation, async-rendered widgets, hydration —
adds capture-time gotchas that surface only at runtime, each of which can hang
the capture or break byte-reproducibility. screencomp never runs a browser, so
these live in *your* capture step; they are collected here because they are easy
to rediscover the hard way.

### Set explicit Playwright timeouts

Outside the `@playwright/test` runner — i.e. the standalone Playwright script
that is the usual capture setup — there is no test config, so `page.click`,
`page.waitForURL`, and `locator.waitFor` default to a timeout of `0`: **wait
forever**. A missing element then hangs the capture indefinitely instead of
failing, indistinguishable from "slow". Set both explicitly right after creating
the page:

```js
page.setDefaultTimeout(15_000);
page.setDefaultNavigationTimeout(30_000);
```

(`expect.configure({ timeout })` only covers assertions, not actions, so it is
not enough on its own.)

### Split the Chromium flags: determinism vs stability

The standard's flag list mixes two concerns. **Determinism** flags change the
*bytes* and are mandatory for byte-reproducibility:

```text
--disable-skia-runtime-opts   # CPU-independent render path: the decisive flag
--disable-gpu --disable-gpu-rasterization
--use-gl=angle --use-angle=swiftshader
--force-color-profile=srgb
--font-render-hinting=none --disable-lcd-text
--hide-scrollbars
```

**Stability / emulation** flags change the *process model* and leave the bytes
identical, so they are safe to add or drop per environment:

```text
--single-process              # OFF for interactive pages (see below)
--disable-dev-shm-usage
--ipc=host
```

`--single-process` is the trap: on a page doing meaningful client-side work the
renderer can fault and, being the only process, take the whole browser down with
it. It is fine for static sites but should be **off** for interactive ones —
dropping it does not change a single byte of output.

### One browser process per viewport

Capturing several viewports in one long-lived browser process can wedge the
second capture partway through. Launch — and fully close — **one browser process
per viewport** (or per project) rather than reusing one across all of them. It is
reliable and, because the bytes do not depend on process lifetime, still
byte-reproducible.

### Make async / animated widgets byte-reproducible

Playwright's `animations: "disabled"` only freezes **CSS** animations and
transitions. A widget that animates via JS-set styles — fade-ins, async-loaded
image layers, canvas redraws — can be caught mid-transition (e.g. at opacity
`0.97` instead of `1.0`), so a large region differs by a few levels between two
runs of the same build. The [reproducibility gate](#reproducibility-gate-verify)
flags this correctly; the fix is to *settle, then pin* the widget before the
shot:

```js
// 1. Wait for the widget to finish loading/animating.
await page.locator(".chart").waitFor({ state: "visible" });
await page.waitForFunction(() => window.__chartReady === true);

// 2. Force its final visual state so nothing is mid-transition at capture.
await page.addStyleTag({
  content: `*, *::before, *::after { transition: none !important; animation: none !important; }`,
});
await page.locator(".chart").evaluate((el) => { el.style.opacity = "1"; });

// 3. Now capture.
await page.locator(".chart").screenshot({ path: out, animations: "disabled" });
```

Re-run `screencomp verify` until it is green; a remaining diff means a widget is
still settling at capture time.

## Local pre-push guard (the strict gate's local half)

Under the [strict gate](#pick-your-gate), CI hard-fails on drift and the developer
owns the baseline. This hook is what makes that ergonomic: it regenerates and lets
you **commit** the new baseline before pushing, so CI stays green on intended
changes and goes red only on ones you missed. `screencomp init` scaffolds it at
`.githooks/pre-push`; [`examples/pre-push`](examples/pre-push) is the copy-paste
template. (Without it — or under CI auto-accept — you can change UI, pass your
whole local gate, and push without ever learning the visual baseline moved.)

The hook fires **only when a pushed change matches the `[guard].paths` globs** in
`screencomp.toml`, so the common push pays nothing — no capture, no Docker. When
a relevant file changes it is deliberately slow: it captures in the same pinned
container as CI so the bytes match. That cost is the point of gating it behind
`[guard].paths`. On a clean comparison it prints one line and lets the push
through; on drift it regenerates the manifest, builds a review gallery, and
**blocks the push**, asking you to review the gallery and commit the regenerated
manifest before pushing again. It never auto-commits. `git push --no-verify`
bypasses it, and it is a no-op under CI.

The "did a relevant file change?" decision is the `scope` subcommand — robust
glob matching instead of fragile shell globbing. It reads `[guard].paths` from
config and a newline-delimited candidate list from stdin, and touches no git,
network, or working-tree state:

```sh
git diff --name-only "$range" | screencomp scope --changed-from - --exit-code
# exit 3 -> a relevant path matched;  exit 0 -> nothing relevant
```

See [`examples/hooks/README.md`](examples/hooks/README.md) for behavior details
and ready-to-paste wiring for lefthook, husky, simple-git-hooks, and a raw
`.git/hooks/pre-push`.

## Exit codes

| Code | Meaning                                                       |
| ---- | ------------------------------------------------------------- |
| `0`  | Success (no differences/problems, or differences without `--exit-code`) |
| `1`  | Runtime error — I/O, invalid input layout, or bad config      |
| `2`  | CLI usage error (unknown flag, missing required argument)     |
| `3`  | Ran successfully but the result is not clean: `classify --exit-code` or `verify` found differences, `doctor --exit-code` found layout problems, or `scope --exit-code` matched a screenshot-relevant path |

Human output goes to stdout; errors go to stderr; the two never mix.

## Configuration

The `comment` and `scope` commands read optional configuration. Resolution
order: `--config <file>` → `$SCREENCOMP_CONFIG` → a `screencomp.toml`
auto-discovered by walking up from the working directory → built-in defaults (so
no file is required). A path given *explicitly* (`--config`/env) that is missing
is a hard error, surfacing a typo; an auto-discovered file is used when present
and ignored when absent. Any file that is found but invalid is always an error.
Auto-discovery means a repo-root `screencomp.toml` is picked up without flags —
so the pre-push guard's `scope` fires even if the hook forgets `--config`.

```toml
# screencomp.toml
[comment]
title = "Visual changes"   # comment heading
marker = "screencomp"       # [A-Za-z0-9_-]; embedded as <!-- marker --> for upserts
show_unchanged = false      # also list unchanged screenshots
embed_limit = 10            # embed images inline when ≤ N shots differ (0 disables)

[guard]                                          # optional local pre-push guard
paths = ["src/**/*.{ts,tsx,css}", "playwright/**"] # globs that trigger a re-capture
platform = "linux-x86_64"                          # platform key to capture/classify under
manifest = "shots/baseline/linux-x86_64.sha256"    # committed digest baseline
gallery  = "shots/review"                          # local review-gallery output dir
```

All `[guard]` fields are optional; with no `paths` the guard never fires. Only
`scope` consumes `paths` — the rest document where the hook template finds the
baseline and writes its review gallery.

## Examples

[`examples/visual-docs.yml`](examples/visual-docs.yml) is a copy-paste GitHub
Actions workflow for a consuming repository: capture screenshots → build a gallery
→ publish to GitHub Pages → post a sticky screenshot-diff comment on pull
requests. [`examples/pre-push`](examples/pre-push) is the local guard that pairs
with it (see [Local pre-push guard](#local-pre-push-guard-the-strict-gates-local-half)).
See [`examples/README.md`](examples/README.md) for prerequisites and the
[gate choice](#pick-your-gate): the strict default pushes nothing and needs no
special token, while the CI-auto-accept opt-in needs a token that can re-trigger
CI once any status check is required, or PRs stall waiting on checks that never
run.

## Development

Requires a Rust toolchain via [rustup](https://rustup.rs); the channel is pinned
in `rust-toolchain.toml`.

```sh
rustup show          # confirm the pinned toolchain
just bootstrap       # install dev tools (nextest, llvm-cov, deny, machete, lefthook) + git hooks
just check           # the complete quality gate (alias: just full-check)
```

Common recipes (`just --list` for all):

| Recipe            | Purpose                                          |
| ----------------- | ------------------------------------------------ |
| `just run -- …`   | Run the CLI                                       |
| `just format` / `fmt-check` | Format / verify formatting              |
| `just typecheck` / `lint` | Type-check / lint (`-D warnings`)         |
| `just check`      | The full quality gate (alias: `full-check`)      |
| `just test` / `test-e2e` | Unit+integration / end-to-end suites       |
| `just test-cov`   | Coverage with an enforced threshold              |
| `just security` / `deps-check` | Advisories / license+bans+unused-deps |
| `just doc`        | Build docs (`-D` rustdoc warnings)               |
| `just build-release` / `dist-build` | Release build / local archive  |
| `just lint-actions` / `lint-docker` | Lint workflows / the Dockerfile |
| `just image` / `image-run -- …`     | Build / run the container image |

### Diagnostics policy

No quality check is allowed to "pass with warnings". Every rule is either an
**error** (fails the command) or **disabled** (silent). `clippy` runs with
`-D warnings`; rustdoc runs with `-D warnings`; formatting and coverage misses
fail their commands. Successful `just` recipes print minimal output; failures
preserve full diagnostics. Noisy inspection lives in separate recipes
(`just doctor`).

### End-to-end testing policy

E2E tests (`tests/e2e.rs`, run by `just test-e2e`) execute the compiled binary
and assert on **critical user journeys** from the user's perspective — exit
codes, stdout/stderr separation, file effects, and the JSON/Markdown contracts —
not just "the binary starts". Smoke checks are a subset, never the whole suite.

## Releasing

Releases are automated with [release-plz](https://release-plz.dev) from
[Conventional Commits](https://www.conventionalcommits.org):

1. Land commits on `main` (`feat:` → minor, `fix:`/`perf:` → patch,
   `!`/`BREAKING CHANGE` → major; `docs`/`test`/`chore`/`ci` don't release).
2. release-plz opens/updates a **release PR** that bumps the version and writes
   the `CHANGELOG`; it auto-merges once CI is green.
3. Merging tags `vX.Y.Z` and cuts the GitHub Release, which triggers:
   - `release.yml` — per-platform archives + `*.sha256`, and (gated) the
     crates.io publish;
   - `docker.yml` — the multi-arch `ghcr.io/nickderobertis/screencomp` image
     (`:X.Y.Z`, `:X.Y`, `:X`, `:latest`).

crates.io publishing is gated on the `PUBLISH_TO_CRATES_IO` repository variable
and the `CARGO_REGISTRY_TOKEN` secret; the automation needs a `RELEASE_PLZ_TOKEN`
PAT so release events trigger the workflows above. Creating a GitHub Release by
hand (or `gh release create`) is a supported fallback.

## License

MIT — see [LICENSE](LICENSE).
