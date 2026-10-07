# screencomp task runner. The gate and test recipes delegate to the Nx project
# graph (nx.json; see AGENTS.md "Quality gate"): each project declares its own
# targets, which call back into the private `_…` recipes below, and the root only
# decides which projects run them. Successful recipes stay quiet; failures keep
# actionable diagnostics. Run `just` (or `just --list`) to see recipes.
#
# Tiers: the gate, test and lint recipes take a trailing `tier` argument.
# `affected` (the default) runs the projects the change since the merge base can
# reach: NX_BASE when set (CI derives it explicitly), else the merge base of HEAD
# with origin/main. `all` is the full sweep over every project, e.g.
# `just check all`.

set shell := ["bash", "-eu", "-o", "pipefail", "-c"]
set windows-shell := ["bash", "-eu", "-o", "pipefail", "-c"]

# Minimum line coverage enforced by the `coverage` aggregate (the `check` gate).
cov_min := "95"
# Pinned lefthook binary fetched by bootstrap / hooks-install when absent.
lefthook_version := "2.1.9"
# Pinned linters for workflow + Dockerfile checks (fetched on demand).
actionlint_version := "1.7.7"
hadolint_version := "2.12.0"
# Pinned tools for the informational performance suite (`bench*`, `profile`).
# Installed on demand by `just bench-tools`; never part of the quality gate.
hyperfine_version := "1.20.0"
critcmp_version := "0.1.8"
samply_version := "0.13.1"

# Show available recipes.
default:
    @just --list

# One-command machine setup (asdf + direnv + toolchain + tools + hooks; idempotent).
setup:
    @bash scripts/setup.sh

# Fast check of whether this machine is set up (no installs, no network).
setup-check:
    @bash scripts/setup-check.sh

# Install developer tooling, the locked Nx install and git hooks (idempotent).
bootstrap: _ensure-tools _ensure-lefthook hooks-install node-modules
    @cargo fetch --locked
    @echo "bootstrap complete"

# Install the locked JavaScript dependencies (Nx, the browser suites) from bun.lock.
node-modules:
    @bash tools/workspace/node-modules.sh

# Fetch locked dependencies and verify the pinned toolchain is active.
sync:
    cargo fetch --locked
    rustup show active-toolchain

# Run the CLI, e.g. `just run -- classify --help`.
run *args:
    cargo run --locked -- {{args}}

# Format the affected projects in place (`just format all` for every project).
[positional-arguments]
format tier="affected":
    @just _nx "$1" -t format

alias fmt := format

# Check formatting without writing.
[positional-arguments]
fmt-check tier="affected":
    @just _nx "$1" -t format-check

# Type-check every target and feature of the affected Rust projects.
[positional-arguments]
typecheck tier="affected":
    @just _nx "$1" -t typecheck

# Lint the affected projects (clippy -D warnings, boundaries, actionlint/shellcheck, hadolint).
[positional-arguments]
lint tier="affected":
    @just _nx "$1" -t lint

alias clippy := lint

# Apply machine-applicable clippy fixes.
clippy-fix:
    cargo clippy --fix --allow-dirty --allow-staged --locked --workspace --all-targets --all-features -- -D warnings

# The affected projects' tests (the Rust suites run under coverage instrumentation).
[positional-arguments]
test tier="affected":
    @just _nx "$1" -t test

# Re-run the crate's in-process tests on change (requires cargo-watch).
test-watch:
    cargo watch -x "nextest run --locked -p screencomp"

# Tests plus the 95% line-coverage aggregate over the crate's sources.
[positional-arguments]
test-cov tier="affected":
    @just _nx "$1" -t test coverage

# End-to-end tests that execute the compiled binary (the `screencomp-e2e` project).
[positional-arguments]
test-e2e tier="affected":
    @just _nx "$1" -t test "--exclude=*,!tag:type:e2e"

# The browser suites in real Chromium (never in the gate), or only `project`.
[positional-arguments]
test-browser tier="affected" project="":
    @just _nx "$1" -t browser-test ${2:+"--exclude=*,!$2"}

# Needs `just node-modules` first; sudo where OS packages are missing.
# Install the Chromium the browser suites drive, with its OS packages.
browser-install:
    cd browser-tests && node_modules/.bin/playwright install --with-deps chromium

# Build API docs, failing on any rustdoc warning.
doc:
    @./tools/workspace/nx run screencomp:doc

# Security advisories + yanked crates.
security:
    cargo deny check advisories

# License, banned/duplicate-crate, source policy, and unused-dependency hygiene.
deps-check:
    cargo deny check bans licenses sources -A license-not-encountered
    cargo machete

# Build under the declared MSRV (the pinned toolchain equals rust-version).
msrv:
    @./tools/workspace/nx run crate-checks:msrv

# Install git hooks into the working copy.
hooks-install: _ensure-lefthook
    lefthook install

# Run the pre-commit hook set on demand.
hooks: _ensure-lefthook
    lefthook run pre-commit

# Debug build.
build:
    cargo build --locked

# Optimized release build.
build-release:
    cargo build --release --locked

# Verify publish metadata and the crate package without uploading anything.
dist-plan:
    cargo publish --locked --dry-run --allow-dirty -p screencomp
    @echo "binary release targets are defined in .github/workflows/release.yml"

# Build and package a release archive + checksum for the host target.
dist-build: build-release
    #!/usr/bin/env bash
    set -euo pipefail
    name=screencomp
    bin="target/release/${name}"
    ver="$("$bin" --version | awk '{print $2}')"
    triple="$(rustc -vV | sed -n 's/^host: //p')"
    stem="${name}-${ver}-${triple}"
    rm -rf "dist/${stem}" && mkdir -p "dist/${stem}"
    cp "$bin" "dist/${stem}/"
    cp README.md LICENSE CHANGELOG.md "dist/${stem}/"
    tar -czf "dist/${stem}.tar.gz" -C dist "${stem}"
    rm -rf "dist/${stem}"
    if command -v sha256sum >/dev/null 2>&1; then
        ( cd dist && sha256sum "${stem}.tar.gz" > "${stem}.tar.gz.sha256" )
    else
        ( cd dist && shasum -a 256 "${stem}.tar.gz" > "${stem}.tar.gz.sha256" )
    fi
    echo "packaged dist/${stem}.tar.gz"

# --- Performance suite (informational; never part of `full-check`) -----------
# Benchmarks are non-deterministic on shared hardware, so they measure rather
# than gate. `just check`/`clippy` already type-check `benches/`, so the bench
# can't rot without a gate phase of its own. Install the tools with `bench-tools`.

# In-process micro-benchmarks (Criterion); saves the `current` baseline for bench-compare.
bench:
    cargo bench --locked --bench commands -- --save-baseline current

# Save current benchmarks as the `base` baseline (run on the comparison point).
bench-base:
    cargo bench --locked --bench commands -- --save-baseline base

# Diff the latest `bench` run against `base` (run `bench-base` first; needs critcmp).
bench-compare:
    critcmp base current

# End-to-end CLI latency for every verb (hyperfine); writes target/bench/results.*.
bench-cli:
    @bash scripts/bench.sh

# Fast smoke check of the CLI benchmark harness (one run, no warmup, no stable numbers).
bench-cli-smoke:
    @bash scripts/bench.sh --dry-run

# Run both benchmark layers (Criterion + hyperfine).
bench-all: bench bench-cli

# Record a sampling profile to find hot spots (samply); see scripts/profile.sh for modes.
profile *args:
    @bash scripts/profile.sh {{args}}

# Install the pinned performance tools (hyperfine, critcmp, samply) onto PATH.
bench-tools:
    #!/usr/bin/env bash
    set -euo pipefail
    declare -A want=([hyperfine]={{hyperfine_version}} [critcmp]={{critcmp_version}} [samply]={{samply_version}})
    missing=()
    for t in "${!want[@]}"; do
        command -v "$t" >/dev/null 2>&1 || missing+=("${t}@${want[$t]}")
    done
    if [ "${#missing[@]}" -eq 0 ]; then
        echo "performance tools already installed"
    elif command -v cargo-binstall >/dev/null 2>&1; then
        cargo binstall --no-confirm "${missing[@]}"
    else
        cargo install --locked "${missing[@]}"
    fi

# Build the consumer container image locally (requires Docker).
image:
    docker build -t screencomp:dev .

# Run the locally built image, e.g. `just image-run -- --version`.
image-run *args:
    docker run --rm screencomp:dev {{args}}

# Lint GitHub Actions workflows and the example (also enforced in CI).
lint-actions: _ensure-actionlint
    actionlint .github/workflows/*.yml examples/*.yml

# Lint the Dockerfile.
lint-docker: _ensure-hadolint
    hadolint Dockerfile

# The quality gate, run by CI after `bootstrap` (AGENTS.md, "Quality gate"); `just check all` sweeps every project.
[positional-arguments]
check tier="affected":
    #!/usr/bin/env bash
    set -euo pipefail
    tier="$1"
    case "$tier" in
        affected)
            base="$(just _base)"
            ./tools/workspace/nx affected --base="$base" -t format-check lint typecheck test build doc coverage supply-chain release-check
            ;;
        all)
            ./tools/workspace/nx run-many -t format-check lint typecheck test build doc coverage supply-chain release-check
            ;;
        *)
            printf "unknown tier '%s': use 'affected' (the default) or 'all'\n" "$tier" >&2
            exit 2
            ;;
    esac
    printf '✓ check passed (%s tier)\n' "$tier"

# Backward-compatible alias for the `check` gate (kept for docs/bench refs).
alias full-check := check

# Orchestrator/pre-push spelling for the same complete deterministic gate.
alias gate := check

# Remove build and release artifacts.
clean:
    cargo clean
    rm -rf dist .nx

# Upgrade dependencies to the latest semver-compatible versions, then re-gate
# with the full sweep (an upgrade can reach every project). May change Cargo.lock
# and bun.lock.
upgrade:
    cargo update
    bun update
    just check all

# Noisy environment report (kept out of the quality gate).
doctor:
    @echo "# toolchain"; rustup show active-toolchain; rustc --version; cargo --version
    @echo "# tools"; for t in asdf direnv just lefthook cargo-nextest cargo-llvm-cov cargo-deny cargo-machete actionlint hadolint docker hyperfine critcmp samply; do printf '%s: ' "$t"; command -v "$t" || echo "missing"; done
    @echo "# installed targets"; rustup target list --installed

# Run Nx targets at a tier: `affected` against the merge base, or `all`.
[positional-arguments]
_nx tier *args:
    #!/usr/bin/env bash
    set -euo pipefail
    tier="$1"
    shift
    case "$tier" in
        affected) base="$(just _base)"; exec ./tools/workspace/nx affected --base="$base" "$@" ;;
        all) exec ./tools/workspace/nx run-many "$@" ;;
        *) echo "unknown tier '$tier': use 'affected' (the default) or 'all'" >&2; exit 2 ;;
    esac

# The affected tier's base: NX_BASE when set, else the merge base of HEAD with
# origin/main. Validated here, before anything interpolates it into a command: a
# git ref or SHA is letters, digits and `. _ / -`, and it must name a commit.
_base:
    #!/usr/bin/env bash
    set -euo pipefail
    if [ -n "${NX_BASE:-}" ]; then
        base="$NX_BASE"
    elif ! base="$(git merge-base origin/main HEAD 2>/dev/null)"; then
        echo "cannot derive the affected tier's base: HEAD has no merge base with origin/main" >&2
        echo "ACTION: run 'git fetch origin main', or set NX_BASE to the ref or commit to diff against" >&2
        exit 1
    fi
    if ! [[ "$base" =~ ^[A-Za-z0-9._/-]+$ ]]; then
        echo "NX_BASE must be a plain git ref or SHA (letters, digits and . _ / - only); got: $base" >&2
        exit 1
    fi
    if ! git rev-parse --verify --quiet "${base}^{commit}" >/dev/null; then
        echo "NX_BASE '$base' names no commit in this checkout" >&2
        echo "ACTION: fetch it (git fetch origin <ref>), or unset NX_BASE to use the merge base with origin/main" >&2
        exit 1
    fi
    printf '%s\n' "$base"

# llmlint: ignore-block[diagnostics_error_or_absent] These compiles need no -D warnings of their own: every gate tier runs the same project's `lint` (clippy -D warnings over the same targets and features, which reports every rustc warning), so a warning already fails the gate, as AGENTS.md's diagnostics policy states; repeating it as RUSTFLAGS here would rebuild every dependency whenever clippy and these builds alternate.
[positional-arguments]
_rust-format crate:
    cargo fmt -p "$1"

[positional-arguments]
_rust-format-check crate:
    cargo fmt -p "$1" --check

[positional-arguments]
_rust-lint crate:
    cargo clippy --locked -p "$1" --all-targets --all-features -- -D warnings

[positional-arguments]
_rust-typecheck crate:
    cargo check --locked -p "$1" --all-targets --all-features

[positional-arguments]
_rust-build crate:
    cargo build --locked -p "$1"

[positional-arguments]
_rust-doc crate:
    RUSTDOCFLAGS="-D warnings" cargo doc --locked --no-deps --all-features -p "$1"

# A crate's suite under coverage instrumentation, with the report deferred: the
# raw profiles land in target/llvm-cov-target, which `_coverage` merges. `binary`
# names a crate whose binary the suite spawns: it is built (instrumented) in the
# same run, and only `crate`'s own tests execute.
[positional-arguments]
_rust-test crate binary="":
    #!/usr/bin/env bash
    set -euo pipefail
    args=(--locked --all-features -p "$1")
    if [ -n "$2" ]; then
        args+=(-p "$2" -E "package($1)")
    fi
    exec cargo llvm-cov --no-report nextest "${args[@]}"

# Empty the shared profile directory before any instrumented suite writes to it,
# so the aggregate never merges a profile an earlier run left behind.
_coverage-clean:
    cargo llvm-cov clean --workspace

# The {{cov_min}}% line-coverage floor, once, over every suite's profiles: the
# crate's own sources, as covered by its in-process suites and the binary e2e
# suite together.
_coverage:
    cargo llvm-cov report --fail-under-lines {{cov_min}} --summary-only

# Supply chain: license/ban/source policy (`license-not-encountered` silenced:
# the allow-list is accepted-license policy, not an inventory of what the tree
# happens to use), unused dependencies, then advisories and yanked crates.
_supply-chain:
    cargo deny check bans licenses sources -A license-not-encountered
    cargo machete
    cargo deny check advisories

# The shipped artifact: the optimized release build and the crate package.
_release-check:
    cargo build --release --locked
    cargo publish --locked --dry-run --allow-dirty -p screencomp

_msrv:
    cargo check --locked -p screencomp --all-features

# The visual-docs contract suite (tests/actions.rs), uninstrumented.
_actions-test:
    cargo nextest run --locked -p screencomp --test actions
# llmlint: ignore-end[diagnostics_error_or_absent]

# actionlint over the reusable workflow, its smoke tests and the documented
# callers; shellcheck over the scripts the actions run (actionlint only lints the
# shell embedded in workflows).
[linux]
_lint-visual-docs: _ensure-actionlint
    actionlint .github/workflows/visual-docs-reusable.yml .github/workflows/test-visual-docs.yml .github/workflows/test-gh-pages-maintenance.yml examples/*.yml
    @command -v shellcheck >/dev/null 2>&1 || { echo "shellcheck is not installed: https://github.com/koalaman/shellcheck#installing" >&2; exit 1; }
    shellcheck scripts/visual-docs-gh-pages.sh scripts/visual-docs-pages-build.sh

# The definition linters read platform-independent files, so one verdict per tree
# suffices, and their pinned binaries are Linux-only in practice (none for
# Windows, a crashing hadolint on Apple silicon): the Linux `check` leg and the
# `workflows` job run them.
[macos]
[windows]
_lint-visual-docs:
    @echo "_lint-visual-docs: actionlint and shellcheck run on Linux (the Linux check leg and the workflows job)"

# hadolint, as the docker-image project's `lint` runs it; Linux-only like
# `_lint-visual-docs`. `just lint-docker` runs it unconditionally.
[linux]
_lint-docker-image: lint-docker

[macos]
[windows]
_lint-docker-image:
    @echo "_lint-docker-image: hadolint runs on Linux (the Linux check leg and the workflows job)"

# The gallery browser suite against the freshly built debug binary.
_browser-test:
    cd browser-tests && PATH="{{justfile_directory()}}/target/debug:$PATH" node_modules/.bin/playwright test

# The demo's own capture spec on the host, from the demo's own npm lockfile
# (demo/ is mirrored onto screencomp-demo, which installs it with npm), into a
# scratch directory. A smoke of the spec, not the pinned-container capture.
_demo-browser-test:
    #!/usr/bin/env bash
    set -euo pipefail
    out="$(mktemp -d)"
    log="$(mktemp)"
    trap 'rm -rf "$out" "$log"' EXIT
    cd demo
    npm ci --no-audit --no-fund >"$log" 2>&1 || { cat "$log" >&2; echo "the demo's 'npm ci' failed; its output is above. ACTION: if demo/package.json changed, run 'npm install' in demo/ and commit demo/package-lock.json; otherwise check access to the npm registry" >&2; exit 1; }
    SHOTS_OUT="$out" npx playwright test
    test -s "$out/captures.json" || { echo "the demo capture wrote no captures.json; ACTION: check demo/tests/screenshots.spec.ts, which writes it to \$SHOTS_OUT" >&2; exit 1; }

# The gate tooling's own tests (tier selection, project boundaries).
_workspace-test:
    node --test tools/workspace/tests/*.test.mjs

# Install missing cargo-based dev tools as prebuilt binaries via cargo-binstall.
# Building these from source compiles them against the pinned toolchain, and the
# latest releases (e.g. cargo-nextest) carry an MSRV newer than what this repo
# pins — so a `cargo install` fallback fails on a clean machine. cargo-binstall
# fetches prebuilt release binaries, which is both faster and toolchain-agnostic;
# bootstrap installs it first if it is absent so the path works from scratch.
_ensure-tools:
    #!/usr/bin/env bash
    set -euo pipefail
    rustup component add rustfmt clippy llvm-tools-preview >/dev/null 2>&1 || true
    missing=()
    for t in cargo-nextest cargo-llvm-cov cargo-deny cargo-machete; do
        command -v "$t" >/dev/null 2>&1 || missing+=("$t")
    done
    if [ "${#missing[@]}" -eq 0 ]; then
        exit 0
    fi
    if ! command -v cargo-binstall >/dev/null 2>&1; then
        echo "installing cargo-binstall (prebuilt-binary installer)"
        curl -fsSL https://raw.githubusercontent.com/cargo-bins/cargo-binstall/main/install-from-binstall-release.sh | bash
        export PATH="${CARGO_HOME:-$HOME/.cargo}/bin:$PATH"
    fi
    # Only ever install the published prebuilt binaries. The `compile` fallback
    # would build the tool from source against the pinned toolchain, and the
    # latest releases carry a newer MSRV — so a failed/rate-limited download must
    # error loudly, not silently compile. Authenticated GitHub API calls
    # (GITHUB_TOKEN, set in CI) avoid the rate-limiting that triggers fallback.
    cargo binstall --no-confirm --disable-strategies compile "${missing[@]}"

# Install the pinned lefthook binary onto PATH if it is missing.
_ensure-lefthook:
    #!/usr/bin/env bash
    set -euo pipefail
    command -v lefthook >/dev/null 2>&1 && exit 0
    # Windows asset names carry a `.exe`; the others do not.
    ext=""
    case "$(uname -s)" in
        Linux) os=Linux ;;
        Darwin) os=MacOS ;;
        MINGW*|MSYS*|CYGWIN*) os=Windows; ext=".exe" ;;
        *) echo "Install lefthook manually for $(uname -s): https://lefthook.dev" >&2; exit 1 ;;
    esac
    case "$(uname -m)" in
        arm64|aarch64) arch=arm64 ;;
        x86_64|amd64) arch=x86_64 ;;
        *) echo "Unsupported architecture $(uname -m) for lefthook auto-install" >&2; exit 1 ;;
    esac
    dest="${CARGO_HOME:-$HOME/.cargo}/bin"
    mkdir -p "$dest"
    url="https://github.com/evilmartians/lefthook/releases/download/v{{lefthook_version}}/lefthook_{{lefthook_version}}_${os}_${arch}${ext}"
    echo "installing lefthook {{lefthook_version}}"
    curl -fsSL "$url" -o "$dest/lefthook${ext}"
    chmod +x "$dest/lefthook${ext}"

# Install the pinned actionlint binary onto PATH if it is missing.
_ensure-actionlint:
    #!/usr/bin/env bash
    set -euo pipefail
    command -v actionlint >/dev/null 2>&1 && exit 0
    case "$(uname -s)" in
        Linux) os=linux ;;
        Darwin) os=darwin ;;
        *) echo "Install actionlint manually for $(uname -s): https://github.com/rhysd/actionlint" >&2; exit 1 ;;
    esac
    case "$(uname -m)" in
        arm64|aarch64) arch=arm64 ;;
        x86_64|amd64) arch=amd64 ;;
        *) echo "Unsupported architecture $(uname -m) for actionlint auto-install" >&2; exit 1 ;;
    esac
    dest="${CARGO_HOME:-$HOME/.cargo}/bin"
    mkdir -p "$dest"
    url="https://github.com/rhysd/actionlint/releases/download/v{{actionlint_version}}/actionlint_{{actionlint_version}}_${os}_${arch}.tar.gz"
    echo "installing actionlint {{actionlint_version}}"
    curl -fsSL "$url" | tar -xz -C "$dest" actionlint

# Install the pinned hadolint binary onto PATH if it is missing.
_ensure-hadolint:
    #!/usr/bin/env bash
    set -euo pipefail
    command -v hadolint >/dev/null 2>&1 && exit 0
    case "$(uname -s)" in
        Linux) os=Linux ;;
        Darwin) os=Darwin ;;
        *) echo "Install hadolint manually for $(uname -s): https://github.com/hadolint/hadolint" >&2; exit 1 ;;
    esac
    # hadolint publishes Linux arm64/x86_64 and Darwin x86_64 only.
    if [ "$os" = "Darwin" ]; then
        arch=x86_64
    else
        case "$(uname -m)" in
            arm64|aarch64) arch=arm64 ;;
            x86_64|amd64) arch=x86_64 ;;
            *) echo "Unsupported architecture $(uname -m) for hadolint auto-install" >&2; exit 1 ;;
        esac
    fi
    dest="${CARGO_HOME:-$HOME/.cargo}/bin"
    mkdir -p "$dest"
    url="https://github.com/hadolint/hadolint/releases/download/v{{hadolint_version}}/hadolint-${os}-${arch}"
    echo "installing hadolint {{hadolint_version}}"
    curl -fsSL "$url" -o "$dest/hadolint"
    chmod +x "$dest/hadolint"

# The LLM-judge tier (llmlint) is never part of `check`: it is non-deterministic
# and drives a coding harness through oneharness (`oneharness.toml` selects it),
# so it stays out of the deterministic, offline gate. The diff-scoped run is the blocking `llmlint` PR
# check (.github/workflows/llmlint.yml). Install it with `just setup-llmlint`.

# Install/refresh the llmlint toolchain (llmlint + oneharness). Idempotent; the
# SessionStart hook runs it through scripts/session-setup.sh.
setup-llmlint:
    @bash scripts/setup-llmlint.sh

# llmlint over the configured set, or over the paths given.
[positional-arguments]
lint-llm *paths:
    @command -v llmlint >/dev/null 2>&1 || { echo "llmlint not installed: run 'just setup-llmlint'" >&2; exit 1; }
    llmlint "$@"

# llmlint scoped to what this branch changed since it forked from BASE (three-dot
# / merge-base semantics): only the changed files, and the judge only on the
# changed lines. Extra arguments go to llmlint. Fetch BASE first if missing.
[positional-arguments]
lint-llm-diff base="origin/main" *args:
    @command -v llmlint >/dev/null 2>&1 || { echo "llmlint not installed: run 'just setup-llmlint'" >&2; exit 1; }
    llmlint --diff --diff-base "$@"

# The deterministic llmlint gate, no model and no credential: the config parses,
# every `llmlint: ignore` names a configured rule, and edited versioned fragments
# bumped `version:`. Pass `--diff-base origin/main` to scope the version check.
[positional-arguments]
lint-llm-validate *args:
    @command -v llmlint >/dev/null 2>&1 || { echo "llmlint not installed: run 'just setup-llmlint'" >&2; exit 1; }
    llmlint validate "$@"
