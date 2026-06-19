#!/usr/bin/env bash
#
# End-to-end CLI latency benchmark. Drives the optimized release binary the way a
# CI pipeline does — one process per command — and measures wall-clock time with
# hyperfine across every verb. This captures the cost that matters in production:
# process startup + parsing the captures.json index + render (and, for gallery,
# copying images), which the in-process Criterion benches (`benches/commands.rs`)
# deliberately exclude.
#
# Usage:
#   scripts/bench.sh            Full run (warmup + adaptive sampling).
#   scripts/bench.sh --dry-run  One run, no warmup — a fast smoke check that the
#                               harness and every command still work (used by CI
#                               and `just`), without depending on stable numbers.
#
# Results: human table on stdout plus machine-readable exports under
# ${BENCH_OUT:-target/bench} (results.json, results.md).
#
# Environment overrides:
#   BENCH_OUT       output directory (default: <repo>/target/bench)
#   BENCH_WARMUP    warmup runs before timing (default: 10)
#   BENCH_PROJECTS  projects in the synthetic tree (default: 8)
#   BENCH_SHOTS     screenshots per project (default: 8)
#   BENCH_KB        size of each PNG in KiB (default: 16)
#   BENCH_KEEP      set to 1 to keep the temp sandbox for inspection

set -euo pipefail

mode="${1:-run}"
case "$mode" in
    run | --dry-run) ;;
    *)
        echo "usage: bench.sh [--dry-run]" >&2
        exit 2
        ;;
esac

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=scripts/bench-lib.sh
. "$repo_root/scripts/bench-lib.sh"

bin="$repo_root/target/release/screencomp"
out="${BENCH_OUT:-$repo_root/target/bench}"
warmup="${BENCH_WARMUP:-10}"
projects="${BENCH_PROJECTS:-8}"
shots="${BENCH_SHOTS:-8}"
kb="${BENCH_KB:-16}"

note() { printf '%s\n' "$*"; }
fail() {
    printf 'FAIL: %s\n' "$*" >&2
    exit 1
}

if ! command -v hyperfine >/dev/null 2>&1; then
    fail "hyperfine not found on PATH. Install dev tools with 'just bench-tools' (or 'cargo binstall hyperfine')."
fi

# A `--dry-run` proves the harness and commands work without spending time on
# statistics; the full run warms up and lets hyperfine sample adaptively.
runs_opt=()
if [[ "$mode" == "--dry-run" ]]; then
    warmup=0
    runs_opt=(--runs 1)
fi

note "» building release binary"
(cd "$repo_root" && cargo build --release --locked --quiet)
[ -x "$bin" ] || fail "release binary not found at $bin"

# Hermetic sandbox: a deterministic screenshot tree built from scratch, so the
# benchmarked verdicts are reproducible and independent of the repo's fixtures.
sandbox="$(mktemp -d)"
cleanup() { [ "${BENCH_KEEP:-0}" = "1" ] || rm -rf "$sandbox"; }
trap cleanup EXIT

baseline="$sandbox/baseline"
current="$sandbox/current"
gallery_out="$sandbox/gallery"
gallery_diff="$sandbox/gallery-diff"

note "» generating tree (${projects} projects × ${shots} shots × ${kb} KiB)"
bench_gen_tree "$baseline" "$current" "$projects" "$shots" "$kb"

mkdir -p "$out"

note "» benchmarking $bin"
# One invocation so a single export holds every command. `--prepare` clears the
# gallery output dirs before each run so every gallery render measures the
# create-from-empty path rather than an idempotent overwrite; the no-op removal
# is harmless for the other commands. `classify --exit-code` returns 3 by design
# when differences exist, so it is wrapped with `|| true` to keep hyperfine from
# treating it as a failure.
hyperfine \
    --warmup "$warmup" "${runs_opt[@]}" \
    --prepare "rm -rf '$gallery_out' '$gallery_diff'" \
    --export-json "$out/results.json" \
    --export-markdown "$out/results.md" \
    -n "version" "'$bin' --version" \
    -n "help" "'$bin' --help" \
    -n "classify" "'$bin' classify --baseline '$baseline' --current '$current'" \
    -n "classify:json" "'$bin' classify --baseline '$baseline' --current '$current' --format json" \
    -n "classify:exit" "'$bin' classify --baseline '$baseline' --current '$current' --exit-code || true" \
    -n "comment" "'$bin' comment --baseline '$baseline' --current '$current'" \
    -n "comment:embed" "'$bin' comment --baseline '$baseline' --current '$current' --gallery-url https://example.test/g" \
    -n "gallery" "'$bin' gallery --input '$current' --output '$gallery_out'" \
    -n "gallery:diff" "'$bin' gallery --input '$current' --baseline '$baseline' --output '$gallery_diff'"

note ""
note "✓ wrote $out/results.json"
note "       $out/results.md"
