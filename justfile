# screencomp task runner. All common operations live here and delegate to Cargo
# or Rust-native tools. Successful recipes stay quiet; failures keep actionable
# diagnostics. Run `just` (or `just --list`) to see recipes.

set shell := ["bash", "-eu", "-o", "pipefail", "-c"]
set windows-shell := ["bash", "-eu", "-o", "pipefail", "-c"]

# Minimum line coverage enforced by `test-cov` and the `check` gate.
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

# Install developer tooling and git hooks (idempotent).
bootstrap: _ensure-tools _ensure-lefthook hooks-install
    @echo "bootstrap complete"

# Fetch locked dependencies and verify the pinned toolchain is active.
sync:
    cargo fetch --locked
    rustup show active-toolchain

# Run the CLI, e.g. `just run -- classify --help`.
run *args:
    cargo run --locked -- {{args}}

# Format the workspace (skill-standard verb; `fmt` is the short alias below).
format:
    cargo fmt --all

# Short alias for `format`.
fmt:
    cargo fmt --all

# Check formatting without writing.
fmt-check:
    cargo fmt --all --check

# Type-check all targets and features.
typecheck:
    cargo check --locked --all-targets --all-features

# Lint with every enabled lint treated as an error (skill-standard verb).
lint:
    cargo clippy --locked --all-targets --all-features -- -D warnings

# Short alias for `lint`.
clippy:
    cargo clippy --locked --all-targets --all-features -- -D warnings

# Apply machine-applicable clippy fixes.
clippy-fix:
    cargo clippy --fix --allow-dirty --allow-staged --locked --all-targets --all-features -- -D warnings

# Unit + integration tests (excludes the slower binary e2e suite).
test:
    cargo nextest run --locked -E 'not binary(e2e)'

# Re-run tests on change (requires cargo-watch).
test-watch:
    cargo watch -x "nextest run --locked -E 'not binary(e2e)'"

# All tests with an enforced line-coverage threshold.
test-cov:
    cargo llvm-cov nextest --locked --all-features --fail-under-lines {{cov_min}} --summary-only

# End-to-end tests that execute the compiled binary.
test-e2e:
    cargo nextest run --locked -E 'binary(e2e)'

# Build API docs, failing on any rustdoc warning.
doc:
    RUSTDOCFLAGS="-D warnings" cargo doc --locked --no-deps --all-features

# Security advisories + yanked crates.
security:
    cargo deny check advisories

# License, banned/duplicate-crate, source policy, and unused-dependency hygiene.
# `license-not-encountered` is silenced: the allow-list is accepted-license
# policy, not an inventory of what the current tree happens to use.
deps-check:
    cargo deny check bans licenses sources -A license-not-encountered
    cargo machete

# Build under the declared MSRV (the pinned toolchain equals rust-version).
msrv:
    cargo check --locked --all-features

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
    cargo publish --locked --dry-run --allow-dirty
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

# Full quality gate (skill-standard `check` verb). Runs `just test` and
# `just test-e2e` plus {{cov_min}}% coverage; CI runs this after `bootstrap`.
check:
    #!/usr/bin/env bash
    set -euo pipefail
    phase() {
        local label="$1"; shift
        local log; log="$(mktemp)"
        printf '▶ %-12s ' "$label"
        if "$@" >"$log" 2>&1; then
            printf 'ok\n'; rm -f "$log"
        else
            printf 'FAILED\n\n'; cat "$log"; rm -f "$log"; exit 1
        fi
    }
    phase fmt        just fmt-check
    phase typecheck  just typecheck
    phase lint       just lint
    phase test       just test
    phase e2e        just test-e2e
    phase coverage   cargo llvm-cov nextest --locked --all-features --fail-under-lines {{cov_min}} --summary-only
    phase deps       cargo deny check bans licenses sources -A license-not-encountered
    phase unused     cargo machete
    phase security   cargo deny check advisories
    phase doc        env RUSTDOCFLAGS=-D\ warnings cargo doc --locked --no-deps --all-features
    phase release    cargo build --release --locked
    phase dist-plan  cargo publish --locked --dry-run --allow-dirty
    printf '\n✓ check passed\n'

# Backward-compatible alias for the `check` gate (kept for docs/bench refs).
full-check: check

# Remove build and release artifacts.
clean:
    cargo clean
    rm -rf dist

# Upgrade dependencies to the latest semver-compatible versions, then re-gate.
# May change Cargo.lock (and, via re-gate, surface tool-version drift).
upgrade:
    cargo update
    just check

# Noisy environment report (kept out of the quality gate).
doctor:
    @echo "# toolchain"; rustup show active-toolchain; rustc --version; cargo --version
    @echo "# tools"; for t in asdf direnv just lefthook cargo-nextest cargo-llvm-cov cargo-deny cargo-machete actionlint hadolint docker hyperfine critcmp samply; do printf '%s: ' "$t"; command -v "$t" || echo "missing"; done
    @echo "# installed targets"; rustup target list --installed

# --- internal helpers -------------------------------------------------------

# Install missing cargo-based dev tools (prefers cargo-binstall when present).
_ensure-tools:
    #!/usr/bin/env bash
    set -euo pipefail
    rustup component add rustfmt clippy llvm-tools-preview >/dev/null 2>&1 || true
    missing=()
    for t in cargo-nextest cargo-llvm-cov cargo-deny cargo-machete; do
        command -v "$t" >/dev/null 2>&1 || missing+=("$t")
    done
    if [ "${#missing[@]}" -gt 0 ]; then
        if command -v cargo-binstall >/dev/null 2>&1; then
            cargo binstall --no-confirm "${missing[@]}"
        else
            cargo install --locked "${missing[@]}"
        fi
    fi

# Install the pinned lefthook binary onto PATH if it is missing.
_ensure-lefthook:
    #!/usr/bin/env bash
    set -euo pipefail
    command -v lefthook >/dev/null 2>&1 && exit 0
    case "$(uname -s)" in
        Linux) os=Linux ;;
        Darwin) os=MacOS ;;
        *) echo "Install lefthook manually for $(uname -s): https://lefthook.dev" >&2; exit 1 ;;
    esac
    case "$(uname -m)" in
        arm64|aarch64) arch=arm64 ;;
        x86_64|amd64) arch=x86_64 ;;
        *) echo "Unsupported architecture $(uname -m) for lefthook auto-install" >&2; exit 1 ;;
    esac
    dest="${CARGO_HOME:-$HOME/.cargo}/bin"
    mkdir -p "$dest"
    url="https://github.com/evilmartians/lefthook/releases/download/v{{lefthook_version}}/lefthook_{{lefthook_version}}_${os}_${arch}"
    echo "installing lefthook {{lefthook_version}}"
    curl -fsSL "$url" -o "$dest/lefthook"
    chmod +x "$dest/lefthook"

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
