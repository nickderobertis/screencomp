# screencomp

Deterministic screenshot tooling for the **visual-docs framework**: classify a
capture against a baseline, render a static HTML gallery, and produce the sticky
pull-request comment — all byte-reproducible and network-free.

`screencomp` is the *publish CLI* a reusable visual-regression workflow calls
after it captures screenshots. Because captures are byte-reproducible, baselines
can be recomputed rather than committed, so repositories avoid binary churn.

## What it does

A capture is a directory holding a JSON index, `captures.json`, plus the PNG
files it references. The capture step (your Playwright job) writes the index and
computes each shot's content hash; `screencomp` treats that hash as the source of
truth and never decodes or re-hashes the PNGs. A shot's identity is its `name`
plus a map of *toggles* (e.g. `theme=dark`, `viewport=desktop`), so the old fixed
"project" dimension is gone — screen size and the like are now just toggles. From
two such captures, `screencomp`:

- **`classify`** — compares `current` against `baseline` and labels each
  shot `added` / `changed` / `removed` / `unchanged` (by content hash).
- **`gallery`** — renders a self-contained `index.html` index of a capture, or a
  before/after **diff gallery** when given a `--baseline` (great for PR previews).
  One screen is a single card with [toggle controls](#toggle-controls-one-card-per-screen),
  not one card per variant.
- **`comment`** — renders the sticky Markdown PR comment body for a
  classification (with a stable HTML marker for upserts).
- **`manifest`** — writes a capture's index as an image-stripped, pretty-printed
  JSON baseline you commit *instead of* the PNGs (`classify`/`comment` accept it
  via `--baseline-manifest`).
- **`verify`** — asserts two captures of the *same* build are byte-identical
  (the [reproducibility gate](#reproducibility-gate-verify)); exits non-zero the
  moment they diverge.
- **`doctor`** — a [preflight](#preflight-doctor) that prints the resolved
  arch subtree and sanity-checks the `captures.json` index before you classify;
  `doctor --env` instead checks the *setup* (is the pre-push guard enabled, does
  the workflow pin match this CLI, is Docker present).
- **`arches`** — prints the project's configured `[capture].arches` (one per
  line, or a JSON array); the CI matrix reads it to fan out one capture lane per
  arch.

It never decodes images — it compares the content hashes recorded in
`captures.json` — so output is deterministic and the tool has no image-codec
dependencies.

Captures always run in a Linux container, so the OS never varies between a
developer and CI — the only thing that changes the content hash is the CPU
architecture. Each command takes an optional `--arch` to compare within a single
`<root>/<arch>/captures.json` subtree (see
[Per-arch comparison](#per-arch-comparison)). The supported arches are declared
once in `screencomp.toml` (`[capture].arches`), so local commands default to your
host arch and CI fans one lane out per arch.

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
checksum-verify. Resolving `latest` calls the unauthenticated GitHub API, which
rate-limits per IP and returns **403 on shared or proxied egress** (CI,
Codespaces, corporate networks); set `GITHUB_TOKEN` to lift the limit, or skip the
lookup entirely by pinning `--version vX.Y.Z`.

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
- uses: nickderobertis/screencomp@v0
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
    uses: nickderobertis/screencomp/.github/workflows/visual-docs-reusable.yml@v0
    with:
      capture-command: |        # MUST write $SHOTS_OUT/captures.json + the PNGs it references
        npm ci
        npx playwright install --with-deps chromium
        npx playwright test
    secrets:
      push-token: ${{ secrets.VISUAL_DOCS_PUSH_TOKEN }}   # optional; see below
```

The workflow takes no arch input: it reads `[capture].arches` from the committed
`screencomp.toml` (via `screencomp arches --format json`) and fans out one capture
lane per arch, each on a matching runner (`arm64` → `ubuntu-24.04-arm`). Add an
arch to that list to gain a lane.

Affected-only monorepos can feed a dynamic project matrix from an upstream job.
The array is computed at runtime (for example, from `nx affected`), so no static
project list is required:

```yaml
jobs:
  affected:
    runs-on: ubuntu-latest
    outputs:
      projects: ${{ steps.projects.outputs.projects }}
    steps:
      - uses: actions/checkout@v4
      - id: projects
        run: echo 'projects=[{"id":"shop","manifest":"shots/baseline/shop/x86_64.json","gallery-title":"Shop"}]' >>"$GITHUB_OUTPUT"
  visual-docs:
    needs: affected
    uses: nickderobertis/screencomp/.github/workflows/visual-docs-reusable.yml@v0
    with:
      projects: ${{ needs.affected.outputs.projects }}
      capture-command: ./scripts/capture-project "$SCREENCOMP_PROJECT"
```

Each project object requires a unique, non-empty `[A-Za-z0-9_-]` `id` and may set
`current`, `verify`, `manifest`, and `gallery-title`. Defaults are
`shots/current/<id>`, `shots/verify/<id>`, and
`shots/baseline/<id>/<arch>.json`. Custom `current` and `verify` roots must stay
beneath `shots/`, which is the tree transferred from capture to report; manifests
may use any traversal-free relative path. The capture command receives
`SCREENCOMP_PROJECT` and a project/arch-specific `SHOTS_OUT`. Every affected
project gets its own reproducibility lane, baseline, gallery path, and sticky
comment. Capture lanes run in parallel, while report lanes are serialized because
they write shared PR and `gh-pages` branches. Projects absent from the runtime
array are not captured or classified.
An empty `projects` array preserves the original single-capture behavior.

##### One combined comment for many projects (`comment-mode: aggregated`)

By default every project posts its own sticky comment, so a monorepo with a dozen
affected projects leaves a dozen comments on the PR — correct, but noisy. Set
`comment-mode: aggregated` to consolidate them into a **single** upserted comment
instead: a combined summary line plus one row per affected project, each with its
added/changed/removed/unchanged counts and a link to its own gallery.

```yaml
  visual-docs:
    needs: affected
    uses: nickderobertis/screencomp/.github/workflows/visual-docs-reusable.yml@v0
    with:
      projects: ${{ needs.affected.outputs.projects }}
      capture-command: ./scripts/capture-project "$SCREENCOMP_PROJECT"
      comment-mode: aggregated   # one combined comment instead of one per project
```

The rendered comment looks like:

> ## Visual changes
>
> **2 projects affected · 1 added · 1 changed · 1 removed**
>
> | Project | Added | Changed | Removed | Unchanged | Gallery |
> |:--------|------:|--------:|--------:|----------:|:--------|
> | app-admin | 0 | 0 | 1 | 1 | [View gallery](#) |
> | app-web | 1 | 1 | 0 | 0 | [View gallery](#) |

Only affected projects appear; unaffected ones are simply absent (never listed as
removed). The single comment upserts in place across pushes via one stable marker
(`screencomp-aggregate`, per arch on a multi-arch repo). The default
`per-project` value keeps every existing consumer's behavior unchanged, and the
per-project galleries and GitHub Pages output are identical either way — only the
PR-comment surface consolidates. Under the hood each project is still classified
independently (`screencomp comment --projects <spec.json>`), so a
[custom-steps caller](#when-your-capture-needs-custom-steps-the-composite-actions)
can compose the same `visual-docs-aggregate` action after its own report lanes.

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

**Host galleries in a dedicated Pages repository.** Set the reusable workflow's
`pages-repository` input to a public `owner/name` repository and pass a
`pages-token` secret with `contents:write` access to it:

```yaml
with:
  pages-repository: your-org/visual-docs-pages
secrets:
  pages-token: ${{ secrets.VISUAL_DOCS_PAGES_TOKEN }}
```

Enable Pages from the `gh-pages` branch in that repository. Canonical galleries,
PR previews, comment links, and cleanup/history maintenance then all use
`https://your-org.github.io/visual-docs-pages`; the source repository's Pages
configuration is untouched. The token is mandatory when `pages-repository` is
set. The Pages repository must be public for GitHub's anonymous image proxy to
render inline before/after thumbnails in PR comments.

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
- a `schedule:` (cron) trigger runs a **prune** job that keeps the most recent
  gallery versions on `gh-pages` (the last 20 commits by default — set
  `gh-pages-history-versions` to keep more or fewer, or `0` to collapse to a
  single fresh commit) and squashes everything older into one base commit,
  discarding the accreted blob history below the window. It's a destructive
  rewrite of the *generated* branch only — nothing bases work on it, and the
  canonical gallery is rebuilt on the next default-branch push. Schedule it at a
  quiet hour.

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

#### Link to the gallery: the Visual Docs badge

Once the canonical gallery is live on Pages, drop a badge in your README so
reviewers can reach it in one click. Paste this Markdown — it renders on GitHub
and links straight to the gallery:

```markdown
[![Visual Docs](https://img.shields.io/badge/Visual%20Docs-gallery-8A2BE2)](https://OWNER.github.io/REPO/ARCH/)
```

The link target follows the [published gallery layout](#per-arch-comparison): a
per-arch setup deploys each arch under its own subpath, so point the badge at one
of them — `https://OWNER.github.io/REPO/x86_64/` (or `arm64`). A project-level
layout (no `[capture].arches`) has no arch segment, so drop `/ARCH/` and link to
`https://OWNER.github.io/REPO/` instead.

Rendered, it links to the live [`screencomp-demo`](https://github.com/nickderobertis/screencomp-demo)
gallery (captured on `x86_64`):

[![Visual Docs](https://img.shields.io/badge/Visual%20Docs-gallery-8A2BE2)](https://nickderobertis.github.io/screencomp-demo/x86_64/)

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
      - uses: nickderobertis/screencomp@v0               # install the CLI
      - uses: nickderobertis/screencomp/visual-docs@v0   # the report half, one step
        with:
          arch: x86_64             # or "" for a project-level layout (no arch subtree)
          fail-on-drift: true      # strict gate (default): fail on unexpected drift
          pages: true
          github-token: ${{ github.token }}
```

The action expects the capture already on disk (`current`, default `shots/current`)
and the CLI installed; it runs the gate, classify, gallery, Pages deploy, and PR
comment. It needs host tools (`gh`, `git`) **and a real git checkout**, so run it
in a host job that consumes the capture as an artifact — never inside your capture
container, whose checkout often lacks `.git` (which breaks the manifest push and
the comment's base-ref diff). Key inputs: `arch` (empty = project-level),
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
screencomp init            # seeds [capture].arches with your host arch
screencomp init --arch x86_64   # or pick the arch explicitly
```

`--arch` defaults to `auto` (your host arch, e.g. `arm64` on Apple Silicon), so the
scaffold matches the machine it is generated on. This scaffolds the
[strict gate](#pick-your-gate) turnkey — the safe path is the one-command one:

- `screencomp.toml` — config, including `[capture].arches` (the single source of
  truth for which arches you maintain) and the `[guard]` globs the pre-push hook uses.
- `.github/workflows/visual-docs.yml` — a caller for the
  [reusable workflow](#batteries-included-reusable-workflow) with `fail-on-drift:
  true`, so CI fails on unexpected drift. It passes no arch — CI reads
  `[capture].arches`.
- `.githooks/pre-push` — the local guard, executable; it detects your host arch at
  runtime, so the one committed hook is correct on every developer's machine. Enable
  it once per clone with `git config core.hooksPath .githooks` — or pass
  `screencomp init --enable-hook` to have `init` run that for you (it otherwise just
  prints the command, and a scaffolded-but-unenabled guard runs nothing). The hook
  passes your host CA bundle into the capture container and masks `node_modules`
  with an anonymous volume, so it survives TLS-intercepting proxies and matches CI's
  clean install.
- the `.gitignore` lines that commit the tiny digest baselines while ignoring
  generated PNGs and galleries.

It never overwrites existing files (pass `--force` to), and appends the
`.gitignore` block idempotently, so it is safe to re-run. After wiring your
capture into both the workflow and the hook, seed the baseline once and commit it
(the `init` output prints the exact command). The gate is strict by default; to
switch to [CI auto-accept](#pick-your-gate) set `fail-on-drift: false` and
`update-manifest: true` in the workflow.

Given two captures — each a directory with a `captures.json` index plus its
PNGs — that resolve to these shots:

```text
baseline: home                          current: home                          # unchanged
baseline: about                         current: about                         # changed
                                        current: pricing                       # added
baseline: home [viewport=mobile]                                               # removed
```

Classify (human, then machine-readable). The human label is the shot's `name`,
plus its toggles in key order when any are set:

```sh
$ screencomp classify --baseline baseline --current current
changed about
added pricing
removed home [viewport=mobile]
added 1 changed 1 removed 1 unchanged 1

$ screencomp classify --baseline baseline --current current --format json
{"entries":[{"name":"about","toggles":{},"status":"changed"},…],"counts":{"added":1,"changed":1,"removed":1,"unchanged":1},"changed":true}
```

Each JSON entry is `{"name":…,"toggles":{…},"status":…}` — the toggle map
identifies which variant of a screen the entry is.

Build a gallery and render the PR comment:

```sh
# Latest gallery (one tree) — e.g. published from the default branch.
screencomp gallery --input current --output public/screenshots --title "UI"

# Before/after diff gallery (current vs baseline) — e.g. a per-PR preview.
screencomp gallery --input current --baseline baseline \
    --focused --output public/pr-123 --title "PR #123 visual diff"

screencomp comment --baseline baseline --current current \
    --gallery-url https://example.github.io/repo/pr-123/ \
    --output comment.md
```

The diff gallery groups shots into Changed (rendered before/after), Added,
Removed, and Unchanged, and copies both captures' images so it is self-contained:
each shot's PNG lands at `<output>/baseline/<image>` and `<output>/current/<image>`,
where `<image>` is the path that shot's entry records in `captures.json`.
`--focused` keeps unchanged screenshots behind a compact disclosure instead of
putting them in the review flow. Each image subtree also receives its source
`captures.json`; a plain gallery writes that index beside `index.html`. A deployed
canonical gallery is therefore a valid `--baseline` root for a later diff. A
plain gallery (no `--baseline`) copies a single capture's images flat at
`<output>/<image>`.

When the diff is small (at most `comment.embed_limit` screenshots differ — 10 by
default) and the comment can resolve an image URL, it embeds the changed shots
inline (changed before/after, added/removed as a single image) and still links to
the full gallery. Larger diffs fall back to a path listing plus the link.
Override the threshold with `--embed-limit <N>` (`0` disables embedding).

The comment resolves each preview image as `<base>/<image>` using that shot's
`image` path from `captures.json`, matching the gallery layout above:

- `--gallery-url <URL>` is the "View full gallery" link and, on its own, derives
  the preview bases from what `gallery` writes. With an image baseline
  (`--baseline`) that is a diff gallery, so `<URL>/baseline/<image>` and
  `<URL>/current/<image>`. With `--baseline-manifest` no baseline PNGs exist, so
  it points "After" at a plain gallery of the current shots (`<URL>/<image>`) and
  omits "Before" rather than emit a baseline URL that would 404.
- `--baseline-url <URL>` / `--current-url <URL>` override either side explicitly,
  each in the plain `<URL>/<image>` layout. This is how manifest mode still shows
  a real before/after diff: point `--baseline-url` at a canonical/main gallery and
  `--current-url` at the per-PR one.

### Toggle controls: one card per screen

A shot varies over user-defined *toggle dimensions* — `theme`, `viewport`,
`density`, … — recorded per shot in `captures.json` (`"toggles": {"theme":
"dark"}`) and declared once in `screencomp.toml` as `[[toggle]]` tables. Instead
of rendering one card per variant (a wall of near-duplicate `home`/`home-dark`/
`home-mobile` thumbnails), the gallery renders **one card per screen `name`** with
a control group per dimension; clicking a toggle swaps the visible image in place:

```toml
# screencomp.toml — one [[toggle]] per dimension your capture varies over.
[[toggle]]
key = "theme"               # required; [A-Za-z0-9_-]; matches a shot's toggle keys
label = "Theme"             # optional display label; defaults to the key
values = ["light", "dark"]  # required, ordered; the first is the gallery default

[[toggle]]
key = "viewport"
label = "Viewport"
values = ["desktop", "mobile"]
```

A control group appears only for a dimension that *distinguishes* a given name —
one with two or more of its declared values actually present among that name's
shots — so a screen captured at a single theme shows no theme control. Dimensions
render in declaration order, each value in its declared order, opening on the first
(the default). A toggle key or value a shot uses but no `[[toggle]]` declares is a
problem [`doctor`](#preflight-doctor) flags. The gallery is a single self-contained
`index.html` (inline CSS/JS, image `src`s relative to the page), so it deploys
as-is to Pages.

`classify --exit-code` returns a non-zero status when differences exist, for
automation that wants a signal without parsing output:

```sh
screencomp classify --baseline baseline --current current --exit-code || echo "changed"
```

`--quiet` suppresses human output (machine-readable `--format json` is
unaffected).

For a subset capture compared with a shared full baseline, scope classification
by a project toggle. Repeat `--include` to form an include-set; a shot matching
any selector is in scope:

```sh
screencomp classify --baseline-manifest shots/baseline/x86_64.json \
  --current shots/current --include project=shop --include project=checkout
```

Baseline-only shots in the included projects are still `removed`; baseline-only
shots in other projects are ignored because they were deliberately not captured.
Added, changed, and unchanged shots are likewise reported only inside the
include-set. Without `--include`, classification is unchanged and compares the
full union.

### Image-free baselines (digest manifest)

Since comparison is by content digest, the baseline pixels are unnecessary — only
the per-shot digests are. `screencomp manifest` writes the capture's index as a
pretty-printed JSON baseline (the `captures.json` schema with each `image` path
stripped, since a baseline commits no PNGs), which you commit *instead of* the
images so the repository never accumulates binary history:

```sh
# Record the current capture as the baseline. With [capture].arches set, --arch
# defaults to the host arch and can be omitted.
screencomp manifest --input shots/current --arch auto \
    --output shots/baseline/x86_64.json

# Later, classify a new capture against that manifest — no baseline images.
screencomp classify --baseline-manifest shots/baseline/x86_64.json \
    --current shots/current --arch auto
```

`--baseline-manifest` is accepted by `classify` and `comment` as a drop-in
alternative to `--baseline <DIR>` (exactly one is required). The manifest is
already arch-specific, so `--arch` then scopes only `--current`. Its
diff in a pull request (old hash → new hash per shot) is an exact, reviewable
record of what changed; render the actual pixels with `gallery` (which still
needs a capture with images). See [`examples/visual-docs.yml`](examples/visual-docs.yml).

### Per-arch comparison

Captures always run in a Linux container, so the OS that renders a screenshot
never varies between a developer and CI — but the same UI rendered on a different
**CPU architecture** still produces byte-different PNGs. So the one dimension you
split on is the arch: give each arch its own subtree (a `captures.json` index plus
its PNGs under `<root>/<arch>/`) and pass `--arch` to compare only within it:

```text
shots/baseline/x86_64/captures.json   (+ the PNGs it references)
shots/baseline/arm64/captures.json
shots/current/x86_64/captures.json
shots/current/arm64/captures.json
```

```sh
# Explicit arch (e.g. one matrix leg per arch in CI):
screencomp classify --baseline shots/baseline --current shots/current \
    --arch x86_64

# `auto` detects the host's own CPU arch, ideal for a local pre-push check:
screencomp classify --baseline shots/baseline --current shots/current \
    --arch auto
```

`--arch` accepts any subtree name; `auto` resolves to the running binary's CPU
arch (`aarch64` is spelled `arm64`). Every command that walks a tree accepts it.
When `[capture].arches` is configured and `--arch` is omitted, commands default to
the host arch (and hard-error if the host arch is not in that list); with no config
and no `--arch`, the root is treated as project-level (no arch layer). For
`comment`, give each arch a distinct `--marker` (and optionally `--title`) so every
arch keeps its own sticky comment:

```sh
screencomp comment --baseline shots/baseline --current shots/current \
    --arch x86_64 \
    --marker screencomp-x86_64 --title "Visual changes (x86_64)"
```

For a many-project monorepo, `comment --projects <spec.json>` renders one combined
comment for every project instead of one each (see
[aggregated mode](#one-combined-comment-for-many-projects-comment-mode-aggregated)).
The spec is a versioned JSON document — each project carries the same inputs the
single-project command takes:

```json
{
  "schema": 1,
  "projects": [
    { "id": "app-web", "current": "shots/current/app-web", "arch": "x86_64",
      "baseline_manifest": "shots/baseline/app-web/x86_64.json",
      "gallery_url": "https://you.github.io/site/pr-7/app-web/x86_64" },
    { "id": "app-admin", "label": "Admin console",
      "baseline": "shots/baseline/app-admin", "current": "shots/current/app-admin",
      "arch": "x86_64", "gallery_url": "https://you.github.io/site/pr-7/app-admin/x86_64" }
  ]
}
```

Each project needs a non-empty `id` (its default row label; `label` overrides) and
exactly one of `baseline`/`baseline_manifest`, plus a `current` root; `arch` and
`gallery_url` are optional. The reusable workflow's `comment-mode: aggregated`
generates this spec for you.

The supported arches live in one place — `[capture].arches` in `screencomp.toml`
(see [Configuration](#configuration)). CI reads that list via `screencomp arches
--format json` to build its capture matrix, and local commands default to your host
arch from it, so the arch set has a single source of truth:

```sh
screencomp arches                 # one arch per line
screencomp arches --format json   # e.g. ["x86_64","arm64"] — the CI matrix
```

Because the comparison is a byte digest, determinism is a *capture-time* concern:
a screenshot's bytes depend on the renderer's CPU, fonts, and GPU. The OS is held
fixed by always capturing inside a pinned Linux container — including on macOS,
where Docker runs a Linux VM, so `--platform=linux/arm64` (or `linux/amd64`)
reproduces the same per-arch pixels as CI. That, plus the decisive
`--disable-skia-runtime-opts` flag (a CPU-independent render path), gives
byte-for-byte reproducibility per arch. Run screencomp inside the container so
`--arch auto` resolves to the container's arch. Native macOS/Windows captures are
deliberately **not** supported — they could never be byte-reproducible between a
developer's machine and CI. See [`examples/visual-docs.yml`](examples/visual-docs.yml)
for the full standard configuration, the deterministic-rendering flags, and a
reproducibility gate.

One caveat sits *below* the arch split: anti-aliased text is byte-reproducible
per machine but not across the different CPUs (Intel vs AMD — both `x86_64`) a
shared runner pool hands out, so on heterogeneous CI a dense-text shot can flip
across re-runs even with a clean `verify`. If your gate flakes that way, see
[Gate flakes across re-runs](#gate-flakes-across-re-runs-cross-cpu-anti-aliasing-drift).

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
screencomp verify --first shots/run-a --second shots/run-b --arch auto
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

Before the first classify, confirm the capture actually landed where every command
looks for it and that its index is well-formed. `doctor` resolves the arch subtree
and reads the `captures.json` index, reporting the screen names, the toggle
dimensions it observed, and the shot count:

```sh
$ screencomp doctor --input shots/current --arch auto
arch: x86_64 (auto)
inspected: shots/current/x86_64/captures.json
names: 2
  home (2 shots)
  about (1 shot)
shots: 3
toggles: 1
  theme [light, dark]
ok: capture index is well-formed
```

It flags the problems that otherwise surface only as a confusing *empty diff* or a
broken gallery: an empty capture (often an `--arch` that does not match the subtree
on disk), a shot using a toggle key or value that no `[[toggle]]` in
`screencomp.toml` declares, and a referenced `image` missing on disk. Any of these
makes the verdict line `problems found: capture index will not render as expected`
instead of `ok: capture index is well-formed`. Pass `--exit-code` to turn a problem
into a non-zero (`3`) status for a CI preflight gate, or `--format json` for a
machine-readable report.

Pass `--baseline-manifest <file>` to also sanity-check a committed manifest
against the capture. `doctor` then warns on the *other* confusing failure — when
**every** shot looks changed — which is almost always a baseline captured on a
different arch than the host, not a real diff. It catches it two ways: an
`<arch>.json` filename naming an arch other than the capture's, and
shared shots with zero unchanged. Both are advisory and never fail the gate.

#### `doctor --env`: is the setup actually wired?

The layout preflight above checks the *capture*; `doctor --env` checks the
*environment* — the class of gap where a repo looks protected but isn't:

```sh
$ screencomp doctor --env
pre-push guard: PRESENT BUT NOT ENABLED — .githooks/pre-push exists but core.hooksPath is unset; run: git config core.hooksPath .githooks
cli version: 0.3.0
workflow pin: v0.3.0 (matches this CLI)
docker: available
problems found: the strict gate's local guard is not active
```

It reports three things and, with `--exit-code`, fails (`3`) on the two that
silently lie:

- **pre-push guard** — a scaffolded `.githooks/pre-push` that was never enabled
  (`core.hooksPath` unset) is the inert-guard trap: the strict gate's local half
  runs nothing. A **problem**. (Run `screencomp init --enable-hook`, or
  `git config core.hooksPath .githooks`.)
- **workflow pin vs CLI** — the scaffolded workflow pins the reusable workflow to
  the version that wrote it; an installed CLI that has since drifted can classify
  differently locally than in CI. A **problem**.
- **docker** — capture needs it; absence is advisory (you may capture elsewhere).

Use `--dir <repo>` to point it at a checkout other than the current directory,
and `--format json` for a machine-readable report. Run it after cloning a repo
that already uses screencomp, or in CI as a setup gate.

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

**The whole loop, when you change something visual:** edit, then `git push`. If
nothing screenshot-relevant changed the hook is a no-op; if it did, the hook
captures and, on drift, regenerates the baseline, builds a review gallery, and
blocks — so you **review the gallery, `git add` the baseline, commit, and `git
push` again**. That is the entire workflow, and it is local-first by design:
under the strict gate **you** own the baseline, so don't wait for CI to "handle"
a visual change. You also don't need to pre-check your environment first. In
particular: you never pick or verify your CPU arch (the hook auto-detects the
host arch, and CI gates its own arch lane independently — local and CI never need
to match), and you never pre-flight Docker (the hook checks for it and fails
loudly with instructions if it's missing). Just run the loop.

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

## Troubleshooting

### Capturing in a containerized, remote, or proxied environment

The scaffolded `docker run` assumes a local Docker daemon and a clean TLS path.
In Claude Code on the web, Codespaces, devcontainers, and corporate networks
neither holds. Run `screencomp doctor --env` first — it tells you whether the
guard is enabled and Docker is reachable. Then:

- **No Docker daemon.** The pre-push guard refuses to run a relevant-change
  capture without one (a green push with no capture would be false assurance).
  Start the daemon, or capture on another machine and let CI gate.
- **TLS-intercepting egress proxy.** A proxy that re-signs HTTPS makes the
  capture container distrust the host's CA, so `npm ci` fails. npm hides this
  behind the cryptic `npm error Exit handler never called!` — re-run with
  `--loglevel verbose` and you'll see `SELF_SIGNED_CERT_IN_CHAIN`. The scaffolded
  hook and [`examples/pre-push`](examples/pre-push) already fix this: they mount
  the host CA bundle (`$NODE_EXTRA_CA_CERTS` / `$SSL_CERT_FILE`) into the
  container and re-export it, a no-op when no such bundle is set. If you hand-roll
  the `docker run`, add `-v "$NODE_EXTRA_CA_CERTS:/host-ca.crt:ro" -e
  NODE_EXTRA_CA_CERTS=/host-ca.crt`.
- **`node_modules` churn.** The hook mounts an anonymous volume at
  `/work/node_modules` so `npm ci` installs cleanly inside the container, matching
  CI's fresh checkout instead of colliding with a host `node_modules` built for a
  different platform.
- **`install.sh` 403.** Resolving `latest` hits the unauthenticated GitHub API,
  which 403s on shared/proxied IPs. Set `GITHUB_TOKEN`, or pin `--version vX.Y.Z`.

### "4 changed" in the PR comment but the gate still passes

There are **two independent comparisons**, and conflating them is a common
first-PR confusion:

- **The committed digest manifest is the pass/fail gate.** `classify --exit-code`
  compares the capture against `shots/baseline/<arch>.json`. Under the
  [strict gate](#pick-your-gate) you regenerate and commit that manifest locally
  (the pre-push guard), so by the time CI runs it already matches — the gate is
  green.
- **The gallery comment is informational before/after.** It diffs against the PR
  *base branch's* gallery/manifest to show what your PR changes. So a PR that
  legitimately changes 4 screenshots shows "4 changed" in the comment *and* a
  green gate — the manifest you committed accounts for those 4, while the comment
  still renders them against `main` for review.

### Gate flakes across re-runs (cross-CPU anti-aliasing drift)

**Symptom.** The same commit's gate passes, then fails on a re-run, with no code
change. [`verify`](#reproducibility-gate-verify) is clean — the capture is
byte-identical when captured twice on one machine — yet `classify` flips
pass/fail across CI re-runs, hitting your **densest-text shots first**.

That signature is the diagnosis:

| `verify` (one machine) | `classify` across re-runs | Cause                          |
| ---------------------- | ------------------------- | ------------------------------ |
| clean (byte-identical) | **flips** pass/fail       | **cross-CPU AA drift** (below) |
| **fails** (diverges)   | flips                     | nondeterministic capture — see [`verify`](#reproducibility-gate-verify) |

**Why.** screencomp's determinism model is **byte-reproducible _per machine_, not
across CPUs for anti-aliased text.** Byte-exact hashing assumes per-pixel
determinism; that holds within one CPU but not across the heterogeneous CPUs a
shared runner pool hands out. `ubuntu-latest` schedules onto Intel *and* AMD;
Blink lays text out in floating point, the two differ in the last bit, and that
occasionally flips an anti-aliased glyph edge by one quantization step (1/255).
Dense text accumulates the most sub-pixel edges, so it drifts first. This is not
font-hinting, coverage, or a capture bug — it is positional float layout, and no
amount of disabling AA fixes it. (The reusable workflow logs the runner CPU on
every capture so "passes then fails" shows up as a visible CPU diff, not a
mystery.)

**The fix ladder** (each step trades cost for robustness; stop at the first that
holds):

1. **Supersample the affected lane** — raise `deviceScaleFactor` to `2` (in your
   Playwright config) on text-dense screens. At higher resolution a sub-pixel
   shift spreads across more AA gradations, so most device pixels stay under the
   1/255 step instead of one flipping. This is an empirical mitigation, not a
   theorem: it held at 2× here (and mobile at 2.625× never drifted), but a
   pathological case — very long lines, more accumulated layout error — can need
   3×. Cost: ~4× the bytes (storage, artifact size, encode/compare time), so
   apply it per-lane, not to pure-graphical UIs that never drift.
2. **Pin the runner CPU** — schedule the capture lane onto a single CPU vendor
   (e.g. a larger/dedicated runner) so the hardware lottery is removed entirely.

Start at step 1; if drift persists, raise the factor or pin the runner. The docs
do **not** promise cross-CPU determinism — they give you the ladder.

### Bootstrapping the first baseline (or a wholesale UI change)

When you introduce screencomp, or rewrite the UI so every shot changes, there is
no committed baseline to pass the gate yet. Seed it once, in the **same arch
container CI uses**, then commit:

```sh
# 1. Capture into shots/current/<arch>/captures.json (your real capture, in the pinned container).
# 2. Confirm the capture is deterministic before you trust it as a baseline:
screencomp verify --first shots/current --second shots/verify --arch auto
# 3. Record the digests as the committed baseline and commit the .json file:
screencomp manifest --input shots/current --arch auto \
  --output shots/baseline/$(screencomp arches | head -1).json
```

`verify` is the determinism check that makes an image-free baseline safe — it
captures-twice-and-asserts-byte-identical, so you don't have to take a single
capture on faith. After committing the manifest, the strict gate has something to
compare against and subsequent PRs gate normally.

## Exit codes

| Code | Meaning                                                       |
| ---- | ------------------------------------------------------------- |
| `0`  | Success (no differences/problems, or differences without `--exit-code`) |
| `1`  | Runtime error — I/O, invalid input layout, or bad config      |
| `2`  | CLI usage error (unknown flag, missing required argument)     |
| `3`  | Ran successfully but the result is not clean: `classify --exit-code` or `verify` found differences, `doctor --exit-code` found layout problems, or `scope --exit-code` matched a screenshot-relevant path |

Human output goes to stdout; errors go to stderr; the two never mix.

## Configuration

Commands read optional configuration — `[capture].arches` (the arch list, also
the default arch for every command), `[comment]`, `[[toggle]]` (the gallery's
toggle dimensions), and `[guard].paths`. `--config`
is a global flag, so it works on any subcommand. Resolution
order: `--config <file>` → `$SCREENCOMP_CONFIG` → a `screencomp.toml`
auto-discovered by walking up from the working directory → built-in defaults (so
no file is required). A path given *explicitly* (`--config`/env) that is missing
is a hard error, surfacing a typo; an auto-discovered file is used when present
and ignored when absent. Any file that is found but invalid is always an error.
Auto-discovery means a repo-root `screencomp.toml` is picked up without flags —
so the pre-push guard's `scope` fires even if the hook forgets `--config`.

```toml
# screencomp.toml
[capture]
# CPU architectures you maintain screenshots for — the single source of truth.
# Captures run in a Linux container, so only the arch varies. Each entry gets its
# own committed baseline (shots/baseline/<arch>.json) and a CI capture lane;
# local commands default to your host arch and require it to be listed here.
arches = ["arm64"]          # or ["x86_64", "arm64"]

[comment]
title = "Visual changes"   # comment heading
marker = "screencomp"       # [A-Za-z0-9_-]; embedded as <!-- marker --> for upserts
show_unchanged = false      # also list unchanged screenshots
embed_limit = 10            # embed images inline when ≤ N shots differ (0 disables)

# One [[toggle]] per dimension your screenshots vary over. The gallery renders a
# control group per dimension that has ≥2 distinct values present for a screen, so
# one screen is a single card you toggle through instead of one card per variant.
[[toggle]]
key = "theme"               # required; [A-Za-z0-9_-]; matches a shot's toggle keys
label = "Theme"             # optional display label; defaults to the key
values = ["light", "dark"]  # required, ordered; the first is the gallery default

[[toggle]]
key = "viewport"
label = "Viewport"
values = ["desktop", "mobile"]

[guard]                                          # optional local pre-push guard
paths = ["src/**/*.{ts,tsx,css}", "playwright/**"] # globs that trigger a re-capture
manifest = "shots/baseline/arm64.json"             # committed digest baseline
gallery  = "shots/review"                          # local review-gallery output dir
```

`[capture].arches` drives both the CI matrix (`screencomp arches --format json`)
and the per-arch default; the baseline manifest filename is `shots/baseline/<arch>.json`.
Each `[[toggle]]` declares a gallery dimension by `key` (matched against a shot's
toggle keys in `captures.json`), an optional `label`, and the ordered `values` it
can take (the first is the default); a toggle key or value a shot uses but no
`[[toggle]]` declares is a problem [`doctor`](#preflight-doctor) flags. All
`[guard]` fields are optional; with no `paths` the guard never fires. Only
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
