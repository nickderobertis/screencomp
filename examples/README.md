# Examples

Copy-paste starting points for adopting `screencomp` in a consuming repository.
These files are templates for *your* repo — they are excluded from the published
crate and are not run by this project's CI.

## `visual-docs.yml`

An end-to-end GitHub Actions workflow that captures screenshots, builds a gallery,
publishes it to GitHub Pages, and posts a sticky screenshot-diff comment on pull
requests. Copy it into `.github/workflows/` and adapt the **Capture screenshots**
step to your stack (Playwright, Cypress, Storybook, …).

> **See it running:** [`screencomp-demo`](https://github.com/nickderobertis/screencomp-demo)
> is a live consumer of this standard — real Playwright captures in the pinned
> container, exercised on every pull request (visual and non-visual changes,
> locally and in CI). It is a good template to copy from.

### Image-free baselines (digest manifest)

The workflow commits only a tiny per-platform **digest manifest**
(`shots/baseline/<platform>.sha256`), never the baseline PNGs. Because
`screencomp` compares by content digest, the manifest is all that `classify` and
`comment` need, so the repository never accumulates binary history and grows
without bound. The manifest's diff in a PR (old hash → new hash per shot) is the
precise record of what changed; the gallery published to Pages shows the current
pixels. Seed it once with `screencomp manifest --input shots/current --platform
auto --output shots/baseline/<platform>.sha256` and commit it.

### Standard configuration: capture in one pinned container

A screenshot's bytes depend on the OS, CPU, fonts, and GPU that rendered it. The
standard configuration fixes all of them by capturing **and** comparing inside a
single pinned `linux/amd64` container — on CI and locally — so captures are
byte-reproducible everywhere and there is one platform key, `linux-x86_64`.

macOS cannot run Linux containers natively (Docker runs a Linux VM), so a
container on a Mac renders Linux pixels; `--platform=linux/amd64` makes them the
same `linux/amd64` pixels as CI (emulated via Rosetta/QEMU). This trades
real-macOS rendering fidelity for exact reproducibility. The decisive flag is
`--disable-skia-runtime-opts`, which forces a CPU-independent render path so
emulated and native amd64 match. See the comments in `visual-docs.yml` for the
full flag list and the one-time command that validates emulated capture against
CI on Apple Silicon.

### Reproducibility gate (required)

Image-free baselines are only safe if capture is deterministic, so the workflow
captures the same build **twice** and requires byte-identical output with
`screencomp verify --first … --second … --platform auto` (exit `3` on any
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

### Branch protection / required status checks

The workflow pushes the regenerated manifest back to the PR branch. GitHub never
starts workflow runs for pushes made with the default `GITHUB_TOKEN`, so after
that bot commit the new head has **no** runs at all — and every required status
check (this workflow or any other, e.g. your test suite) sits at "Expected —
Waiting for status to be reported" until the branch moves again. Without
required checks this is harmless (and saves a re-capture); with branch
protection it blocks the PR.

Two ways to run under branch protection:

- **Set the `VISUAL_DOCS_PUSH_TOKEN` secret** to a credential that can trigger
  workflows — a fine-grained PAT or a GitHub App installation token with
  `contents: read/write` on the repository. The checkout step falls back to
  `GITHUB_TOKEN` when the secret is absent, so the same file serves both setups.
  The bot push then re-runs CI on the updated head. This cannot loop: the
  re-triggered run regenerates an identical manifest, finds no diff, and does
  not push, so it converges after one extra run.
- **Delete the "Update the baseline manifest" step** and commit the manifest
  yourself when visuals change — the [local pre-push guard](#local-pre-push-guard-optional)
  regenerates it on your machine and blocks the push until you do. This keeps
  the workflow on the default token at the cost of requiring the hook (or a
  manual `screencomp manifest` run) whenever the baseline moves.

A PR already stuck this way recovers as soon as its branch is pushed with real
user credentials — for example, fold the bot's manifest commit into your own
commit and push.

Prerequisites:

- A committed baseline manifest at `shots/baseline/linux-x86_64.sha256` (text).
- The capture step writes the current run to
  `shots/current/linux-x86_64/<project>/<name>.png` inside the pinned container.
- GitHub Pages enabled (**Settings → Pages → Build and deployment → GitHub Actions**).

## Local pre-push guard (optional)

[`pre-push`](pre-push) is a copy-paste Git hook that **complements** the CI
workflow (it does not replace it). CI silently regenerates the digest manifest on
every PR, so without a local guard you can change UI, pass your whole local gate,
and push without ever learning the visual baseline moved. The hook closes that
gap on your machine.

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
[guard]
paths = ["src/**/*.{ts,tsx,css}", "playwright/**", "public/**"]
platform = "linux-x86_64"
manifest = "shots/baseline/linux-x86_64.sha256"
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
