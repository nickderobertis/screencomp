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

It never decodes images — it content-hashes bytes — so output is deterministic
and the tool has no image-codec dependencies.

## Install

### From release binaries

Prebuilt archives (with SHA-256 checksums) are attached to each
[GitHub Release](https://github.com/nickderobertis/screencomp/releases) for Linux
(x86_64, arm64), macOS (x86_64, arm64), and Windows (x86_64). Download the
archive for your platform, verify the checksum, extract, and place `screencomp`
on your `PATH`.

### From crates.io

```sh
cargo install screencomp --locked
```

### From source

```sh
git clone https://github.com/nickderobertis/screencomp
cd screencomp
just build-release   # binary at target/release/screencomp
```

### As a GitHub Action

A composite action installs the CLI (and optionally runs it) on a runner:

```yaml
- uses: nickderobertis/screencomp@v1
  with:
    version: latest        # or a pinned tag like v0.1.0
- run: screencomp classify --baseline shots/baseline --current shots/current
```

Inputs: `version` (release tag or `latest`), `method` (`download` prebuilt asset,
the default, or `source` to `cargo install`), and `args` (run `screencomp <args>`
right after install). Downloads are verified against their published SHA-256.

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
Removed, and Unchanged, and copies both image trees so it is self-contained.

When `--gallery-url` is given and the diff is small (at most `comment.embed_limit`
screenshots differ — 10 by default), the comment embeds the changed shots inline
(changed before/after, added/removed as a single image) resolved against that
URL, and still links to the full gallery. Larger diffs fall back to a path
listing plus the link. Override the threshold with `--embed-limit <N>` (`0`
disables embedding).

`classify --exit-code` returns a non-zero status when differences exist, for
automation that wants a signal without parsing output:

```sh
screencomp classify --baseline baseline --current current --exit-code || echo "changed"
```

`--quiet` suppresses human output (machine-readable `--format json` is
unaffected).

## Exit codes

| Code | Meaning                                                       |
| ---- | ------------------------------------------------------------- |
| `0`  | Success (no differences, or differences without `--exit-code`)|
| `1`  | Runtime error — I/O, invalid input layout, or bad config      |
| `2`  | CLI usage error (unknown flag, missing required argument)     |
| `3`  | `classify --exit-code` ran successfully and found differences |

Human output goes to stdout; errors go to stderr; the two never mix.

## Configuration

The `comment` command reads optional configuration. Resolution order:
`--config <file>` → `$SCREENCOMP_CONFIG` → built-in defaults (so no file is
required). A path given explicitly that is missing or invalid is a hard error.

```toml
# screencomp.toml
[comment]
title = "Visual changes"   # comment heading
marker = "screencomp"       # [A-Za-z0-9_-]; embedded as <!-- marker --> for upserts
show_unchanged = false      # also list unchanged screenshots
embed_limit = 10            # embed images inline when ≤ N shots differ (0 disables)
```

## Examples

[`examples/visual-docs.yml`](examples/visual-docs.yml) is a copy-paste GitHub
Actions workflow for a consuming repository: capture screenshots → build a gallery
→ publish to GitHub Pages → post a sticky screenshot-diff comment on pull
requests. See [`examples/README.md`](examples/README.md) for prerequisites.

## Development

Requires a Rust toolchain via [rustup](https://rustup.rs); the channel is pinned
in `rust-toolchain.toml`.

```sh
rustup show          # confirm the pinned toolchain
just bootstrap       # install dev tools (nextest, llvm-cov, deny, machete, lefthook) + git hooks
just full-check      # the complete quality gate
```

Common recipes (`just --list` for all):

| Recipe            | Purpose                                          |
| ----------------- | ------------------------------------------------ |
| `just run -- …`   | Run the CLI                                       |
| `just fmt` / `fmt-check` | Format / verify formatting                 |
| `just check` / `clippy`  | Type-check / lint (`-D warnings`)          |
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
