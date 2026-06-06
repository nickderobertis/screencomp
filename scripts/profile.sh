#!/usr/bin/env bash
#
# Sampling profiler for finding hot spots, built on samply (records a trace you
# open in the Firefox Profiler UI).
#
# Both modes build the dedicated `profiling` profile (Cargo.toml): the shipped
# release optimizations, but with symbols kept so samply can attribute time to
# functions. The real `[profile.release]` artifact stays stripped.
#
# Usage:
#   scripts/profile.sh                       Profile the whole command pipeline
#                                            (the `commands` bench).
#   scripts/profile.sh engine [FILTER]       Profile one or more Criterion
#                                            benchmarks (e.g. classify/large).
#   scripts/profile.sh cli [VERB]            Profile a real CLI invocation
#                                            (default: classify) against a
#                                            generated tree, looped so the
#                                            short-lived process yields samples.
#
# A single CLI run is far too short to sample, which is why the engine mode
# (Criterion's `--profile-time`, a long-running in-process loop) is the right
# tool for optimizing the in-process work, and the CLI mode loops the binary to
# capture startup + walk + hash + render together.
#
# Environment overrides:
#   PROFILE_SECONDS   engine mode: seconds to sample (default: 10)
#   PROFILE_REPEAT    cli mode: invocations to loop under the profiler (default: 2000)
#   PROFILE_PROJECTS  cli mode: projects in the generated tree (default: 8)
#   PROFILE_SHOTS     cli mode: shots per project (default: 8)
#   PROFILE_KB        cli mode: size of each PNG in KiB (default: 16)
#   SAMPLY_ARGS       extra args passed to `samply record` (e.g. --save-only)

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=scripts/bench-lib.sh
. "$repo_root/scripts/bench-lib.sh"

seconds="${PROFILE_SECONDS:-10}"
repeat="${PROFILE_REPEAT:-2000}"
# shellcheck disable=SC2206  # intentional word-splitting of optional flags.
samply_args=(${SAMPLY_ARGS:-})

fail() {
    printf 'FAIL: %s\n' "$*" >&2
    exit 1
}

command -v samply >/dev/null 2>&1 ||
    fail "samply not found on PATH. Install dev tools with 'just bench-tools' (or 'cargo install --locked samply')."

mode="${1:-engine}"

if [[ "$mode" == "engine" ]]; then
    shift || true
    filter="${1:-}"
    echo "» building bench (profiling profile)"
    # Build the bench with symbols, then read its executable path from cargo's
    # JSON output (no jq dependency).
    artifact="$(cargo build --profile profiling --bench commands --locked --message-format=json -q |
        grep -F '"name":"commands"' | grep -F '"executable":' | tail -1)"
    bench_exe="$(printf '%s' "$artifact" | grep -o '"executable":"[^"]*"' | cut -d'"' -f4)"
    [ -n "$bench_exe" ] && [ -x "$bench_exe" ] || fail "could not locate the profiling bench executable"
    echo "» profiling for ${seconds}s (${filter:-all benchmarks})"
    # `--profile-time` makes Criterion run the bench in a plain loop with no
    # statistical analysis — exactly what an external sampler wants.
    samply record "${samply_args[@]}" -- \
        "$bench_exe" --bench --profile-time "$seconds" ${filter:+"$filter"}
    exit 0
fi

if [[ "$mode" == "cli" ]]; then
    shift || true
    verb="${1:-classify}"
    case "$verb" in
        classify | gallery | comment) ;;
        *) fail "cli verb must be one of: classify, gallery, comment (got '$verb')" ;;
    esac

    bin="$repo_root/target/profiling/screencomp"
    echo "» building binary (profiling profile)"
    (cd "$repo_root" && cargo build --profile profiling --locked --quiet)
    [ -x "$bin" ] || fail "profiling binary not found at $bin"

    sandbox="$(mktemp -d)"
    trap 'rm -rf "$sandbox"' EXIT
    baseline="$sandbox/baseline"
    current="$sandbox/current"
    echo "» generating tree"
    bench_gen_tree "$baseline" "$current" \
        "${PROFILE_PROJECTS:-8}" "${PROFILE_SHOTS:-8}" "${PROFILE_KB:-16}"

    case "$verb" in
        classify) args=(classify --baseline "$baseline" --current "$current") ;;
        comment) args=(comment --baseline "$baseline" --current "$current") ;;
        gallery) args=(gallery --input "$current" --baseline "$baseline" --output "$sandbox/out") ;;
    esac

    echo "» profiling '$bin ${verb}' over $repeat invocations"
    samply record "${samply_args[@]}" -- \
        bash -c 'n="$1"; shift; for ((i = 0; i < n; i++)); do "$@" >/dev/null 2>&1 || true; done' \
        _ "$repeat" "$bin" "${args[@]}"
    exit 0
fi

fail "unknown mode '$mode' (expected: engine | cli)"
