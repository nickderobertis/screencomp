# Examples

Copy-paste starting points for adopting `screencomp` in a consuming repository.
These files are templates for *your* repo — they are excluded from the published
crate and are not run by this project's CI.

## `visual-docs.yml` vs `visual-docs-custom.yml`

Two copy-paste workflows for `.github/workflows/`; pick by how much of the capture
you need to control:

- [`visual-docs.yml`](visual-docs.yml) — **raw CLI**: each step calls the
  `screencomp` binary directly and deploys via the GitHub Actions Pages source. A
  good read for understanding the whole pipeline with no abstraction.
- [`visual-docs-custom.yml`](visual-docs-custom.yml) — **thin glue over the
  composable actions** (`screencomp`, `screencomp/visual-docs`,
  `screencomp/gh-pages-maintenance`). It is the reusable workflow written out as
  plain jobs, so you own only the capture steps and can inject OIDC/private-registry
  auth (it shows AWS CodeArtifact). Prefer this when the reusable workflow's
  string `capture-command` can't host the steps you need.

If you need neither — just adapt the **Capture screenshots** step — start from
`visual-docs.yml` (Playwright, Cypress, Storybook, …).

> **See it running:** [`screencomp-demo`](https://github.com/nickderobertis/screencomp-demo)
> is a live consumer of this standard — real Playwright captures in the pinned
> container, exercised on every pull request (visual and non-visual changes,
> locally and in CI). It is a good template to copy from.

### Image-free baselines (digest manifest)

The workflow commits only a tiny per-arch **digest manifest**
(`shots/baseline/<arch>.sha256`), never the baseline PNGs. Because
`screencomp` compares by content digest, the manifest is all that `classify` and
`comment` need, so the repository never accumulates binary history and grows
without bound. The manifest's diff in a PR (old hash → new hash per shot) is the
precise record of what changed; the gallery published to Pages shows the current
pixels. Seed it once with `screencomp manifest --input shots/current --arch
auto --output shots/baseline/<arch>.sha256` and commit it.

### Standard configuration: capture in one pinned container

A screenshot's bytes depend on the OS, CPU, fonts, and GPU that rendered it.
Pinning capture to a single Linux container — on CI and locally — fixes the OS
everywhere, so the ONLY dimension left is the CPU **arch**. That is the single
source of truth declared once in `screencomp.toml` (`[capture].arches`), with one
subtree and one baseline per arch (`shots/baseline/<arch>.sha256`).

macOS cannot run Linux containers natively (Docker runs a Linux VM), so a
container on a Mac renders Linux pixels; `--platform=linux/amd64` (or
`linux/arm64`) makes them the same per-arch pixels as CI (emulated via
Rosetta/QEMU). This trades real-macOS rendering fidelity for exact
reproducibility. Native macOS/Windows captures are NOT supported — they could
never be byte-reproducible against CI. The decisive flag is
`--disable-skia-runtime-opts`, which forces a CPU-independent render path so
emulated and native captures of the same arch match. See the comments in
`visual-docs.yml` for the full flag list and the one-time command that validates
emulated capture against CI on Apple Silicon.

### Reproducibility gate (required)

Image-free baselines are only safe if capture is deterministic, so the workflow
captures the same build **twice** and requires byte-identical output with
`screencomp verify --first … --second … --arch auto` (exit `3` on any
divergence). Treat this as a required step, not an optional one: it is what turns
a flaky, JS-animated, or async-rendered widget from a silent baseline poisoner
into a hard, fixable failure. The README's
[Capturing an interactive app](https://github.com/nickderobertis/screencomp#capturing-an-interactive-app)
covers the usual causes and fixes.

On Apple Silicon, enable Docker Desktop's **Rosetta** for amd64 emulation. Under
the QEMU fallback, `--use-angle=swiftshader` can crash Chromium
(`qemu: uncaught target signal`); a CPU-rasterization fallback (`--disable-gpu`,
one browser per context, keeping `--disable-skia-runtime-opts`) is byte-identical
to native CI — see `screencomp-demo` for a worked configuration.

### Pick your gate: strict (recommended) vs CI auto-accept

The workflow ships the **strict gate** by default: `classify --exit-code` FAILS
the job when a capture drifts from the committed baseline, and you regenerate and
commit the baseline locally (the [pre-push guard](#local-pre-push-guard-the-strict-gates-local-half))
before pushing. CI goes red only on drift you missed. Because nothing is pushed
back to the branch, this needs no elevated token and never trips the
required-status-check problem below — it just works under branch protection.

The lighter alternative is **CI auto-accept**: drop `--exit-code` and re-enable
the "Update the baseline manifest" step so CI regenerates and pushes the manifest
to the PR branch for you (no red check on drift). The trade-off is that an
unintended visual change can slip through unnoticed, which is exactly why the
strict gate is the default.

### Branch protection (only relevant to CI auto-accept)

If you opt into CI auto-accept, the workflow pushes the regenerated manifest back
to the PR branch. GitHub never starts workflow runs for pushes made with the
default `GITHUB_TOKEN`, so after that bot commit the new head has **no** runs at
all — and every required status check (this workflow or any other, e.g. your test
suite) sits at "Expected — Waiting for status to be reported" until the branch
moves again. Without required checks this is harmless; with branch protection it
blocks the PR.

Two ways to run CI auto-accept under branch protection:

- **Set the `VISUAL_DOCS_PUSH_TOKEN` secret** to a credential that can trigger
  workflows — a fine-grained PAT or a GitHub App installation token with
  `contents: read/write` on the repository. The checkout step falls back to
  `GITHUB_TOKEN` when the secret is absent, so the same file serves both setups.
  The bot push then re-runs CI on the updated head. This cannot loop: the
  re-triggered run regenerates an identical manifest, finds no diff, and does
  not push, so it converges after one extra run.
- **Use the strict gate instead** and commit the manifest yourself when visuals
  change — the [local pre-push guard](#local-pre-push-guard-the-strict-gates-local-half)
  regenerates it on your machine and blocks the push until you do. This keeps the
  workflow on the default token (the recommended path).

A PR already stuck this way recovers as soon as its branch is pushed with real
user credentials — for example, fold the bot's manifest commit into your own
commit and push.

Prerequisites:

- A committed baseline manifest at `shots/baseline/x86_64.sha256` (text), one per
  arch in `[capture].arches`.
- The capture step writes the current run to
  `shots/current/<arch>/<project>/<name>.png` inside the pinned container.
- GitHub Pages enabled (**Settings → Pages → Build and deployment → GitHub Actions**).

## Local pre-push guard (the strict gate's local half)

[`pre-push`](pre-push) is a copy-paste Git hook — the local half of the strict
gate. CI hard-fails on drift; this hook lets you regenerate and **commit** the new
baseline before pushing, so CI stays green on intended changes and goes red only
on ones you missed. Without it (or under CI auto-accept) you can change UI, pass
your whole local gate, and push without ever learning the visual baseline moved.
The hook closes that gap on your machine, before CI runs. `screencomp init`
scaffolds it for you at `.githooks/pre-push`; it detects the host arch at runtime,
so the one committed hook is correct on every developer's machine.

It fires **only when a pushed change matches the `[guard].paths` globs** in
`screencomp.toml`, so most pushes pay nothing. When a relevant file changes it is
deliberately slow — it captures in the same pinned Docker container as CI so the
bytes match — which is exactly why it runs only when needed. On drift it
regenerates the manifest, builds a review gallery, and **blocks the push** with
instructions; it never auto-commits, so you review the gallery and commit the
regenerated manifest yourself before pushing again. `git push --no-verify`
bypasses it, and it is a no-op under CI.

The relevance check is delegated to `screencomp scope`, which matches the changed
paths against `[guard].paths` (robust string matching, no git/network/working-tree
access) rather than fragile shell globbing. Configure it alongside `[comment]`:

```toml
[capture]
arches = ["x86_64"]   # the arch(es) you maintain; also the per-command default

[guard]
paths = ["src/**/*.{ts,tsx,css}", "playwright/**", "public/**"]
manifest = "shots/baseline/x86_64.sha256"
gallery  = "shots/review"
```

See [`hooks/README.md`](hooks/README.md) for behavior details and ready-to-paste
wiring for lefthook, husky, simple-git-hooks, and a raw `.git/hooks/pre-push`.

## Installing the CLI

The workflow installs the CLI with the bundled composite action:

```yaml
- uses: nickderobertis/screencomp@v1
  with:
    version: latest # or a pinned tag like v0.1.0
```

Other options, outside Actions:

```sh
# Recommended: prebuilt binary, checksum-verified, onto your PATH
curl -fsSL https://raw.githubusercontent.com/nickderobertis/screencomp/main/scripts/install.sh | sh

# With cargo, from git (the crate is not yet published to crates.io)
cargo install --git https://github.com/nickderobertis/screencomp --locked screencomp

# Or download a release archive by hand (Linux/macOS/Windows binaries + checksums)
#   https://github.com/nickderobertis/screencomp/releases
```

## Running via the container image

A multi-arch image is published to GitHub Container Registry. Mount your
screenshots and run any subcommand; pass `--user` so files written back to the
host are owned by you:

```sh
docker run --rm \
  --user "$(id -u):$(id -g)" \
  -v "$PWD:/work" \
  ghcr.io/nickderobertis/screencomp:latest \
  gallery --input shots/current --output site --title "Visual docs"
```

The image's entrypoint is `screencomp`, so the trailing arguments are the
subcommand and its flags.
