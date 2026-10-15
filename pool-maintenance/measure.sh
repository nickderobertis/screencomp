#!/usr/bin/env bash
# The measurement behind `screencomp-pool-maintenance-seconds` (budgets.yaml):
# one steady-state pass of the `pool-maintenance` recipe onevcs.toml declares,
# the pass a regular post-run maintenance faces.
#
#   (a) one untimed pass clears any backlog;
#   (b) the gate's test build under an alternative profile setting gives every
#       crate new metadata hashes, and exactly the entries it created are
#       backdated past the 2-day window: one run's builds since superseded;
#   (c) the ordinary gate build makes each unit's newest build the ordinary one;
#   (d) one timed pass removes the alternative builds.
#
# Only (d) reports the figure, through cargo-prune-superseded's own
# onebudgetspec report; the backlog pass's seconds are appended to its detail
# as separate evidence. The script fails, rather than reporting a meaningless
# figure, unless (d) removed at least one build and every entry it removed is
# one (b) created. The entry lists it compares stay in target/pool-maintenance-budget/.
#
# Three variables let pool-maintenance/maintenance.test.mjs drive this script
# against a small Cargo fixture without building the workspace:
# MEASURE_WORKSPACE is the directory whose target dir is measured (default the
# repository root), MEASURE_GATE_BUILD is a shell command run there in place of
# the gate's test build, and MEASURE_EVIDENCE is where the entry lists and logs
# go (default target/pool-maintenance-budget under the workspace). The declared
# recipe always comes from this repository's justfile.
set -euo pipefail
repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${MEASURE_WORKSPACE:-$repo}"

if ! command -v cargo-prune-superseded >/dev/null 2>&1; then
    echo "measure.sh: cargo-prune-superseded is not on PATH" >&2
    echo "ACTION: run 'just bootstrap', which installs the pinned release" >&2
    exit 1
fi
result="${ONEBUDGETSPEC_RESULT:-}"
evidence="${MEASURE_EVIDENCE:-target/pool-maintenance-budget}"
rm -rf "$evidence"
mkdir -p "$evidence"

# Every build entry the pruner selects among: the children of deps/,
# .fingerprint/, build/ and incremental/ in each profile directory (one holding
# .fingerprint/), sorted.
entries() {
    [ -d target ] || return 0
    find target -type d -name .fingerprint -prune -print | while IFS= read -r fingerprint; do
        profile="$(dirname "$fingerprint")"
        for kind in deps .fingerprint build incremental; do
            if [ -d "$profile/$kind" ]; then
                find "$profile/$kind" -mindepth 1 -maxdepth 1
            fi
        done
    done | LC_ALL=C sort
}

# Entry paths reduced to their sorted, unique `<unit>-<metadata hash>` names.
by_hash() {
    sed -E 's#.*/##; s/\..*$//' "$1" | LC_ALL=C sort -u
}

# The gate's test build: both Rust suites' `test` targets, through the recipe
# they run (cargo llvm-cov cannot compile them without running them).
gate_build() {
    if [ -n "${MEASURE_GATE_BUILD:-}" ]; then
        bash -c "$MEASURE_GATE_BUILD"
        return
    fi
    just --justfile "$repo/justfile" --working-directory "$repo" _rust-test screencomp
    just --justfile "$repo/justfile" --working-directory "$repo" _rust-test screencomp-e2e screencomp
}

# The declared maintenance, run in the measured workspace.
maintain() {
    just --justfile "$repo/justfile" --working-directory . pool-maintenance
}

now() { date +%s.%N; }

# Replay a failed step's captured log, then name the step and what to do next.
fail_step() {
    cat "$evidence/$1.log" >&2
    echo "measure.sh: the $2 failed (log above, kept in $evidence/$1.log)" >&2
    echo "ACTION: $3, then rerun ./measure.sh" >&2
    exit 1
}


started="$(now)"
(unset ONEBUDGETSPEC_RESULT; maintain) >"$evidence/backlog.log" 2>&1 || fail_step backlog "untimed backlog pass" "fix the error 'just pool-maintenance' reports above"
backlog_seconds="$(awk -v a="$started" -v b="$(now)" 'BEGIN { printf "%.3f", b - a }')"


entries >"$evidence/before-alternative.txt"
CARGO_PROFILE_DEV_DEBUG=line-tables-only gate_build >"$evidence/alternative-build.log" 2>&1 || fail_step alternative-build "alternative-profile test build" "fix the build error above (reproduce with CARGO_PROFILE_DEV_DEBUG=line-tables-only just _rust-test screencomp)"
entries >"$evidence/after-alternative.txt"
LC_ALL=C comm -13 "$evidence/before-alternative.txt" "$evidence/after-alternative.txt" >"$evidence/alternative.txt"
if [ ! -s "$evidence/alternative.txt" ]; then
    echo "measure.sh: the alternative build created no new build entries, so there is nothing superseded to time; inspect $evidence/alternative-build.log and confirm the alternative profile differs from .cargo/config.toml" >&2
    exit 1
fi
while IFS= read -r entry; do
    find "$entry" -exec touch -h -d '2000-01-01T00:00:00Z' {} +
done <"$evidence/alternative.txt"


gate_build >"$evidence/ordinary-build.log" 2>&1 || fail_step ordinary-build "ordinary gate test build" "fix the build error above (reproduce with just _rust-test screencomp)"
entries >"$evidence/before-pass.txt"
LC_ALL=C comm -23 "$evidence/before-pass.txt" "$evidence/alternative.txt" >"$evidence/ordinary.txt"


maintain >"$evidence/timed-pass.log" 2>&1 || fail_step timed-pass "timed maintenance pass" "fix the error 'just pool-maintenance' reports above"
entries >"$evidence/after-pass.txt"
LC_ALL=C comm -23 "$evidence/before-pass.txt" "$evidence/after-pass.txt" >"$evidence/removed.txt"
by_hash "$evidence/ordinary.txt" >"$evidence/ordinary-hashes-before.txt"
LC_ALL=C comm -12 "$evidence/ordinary.txt" "$evidence/after-pass.txt" >"$evidence/ordinary-kept.txt"
by_hash "$evidence/ordinary-kept.txt" >"$evidence/ordinary-hashes-after.txt"

removed_ordinary="$(LC_ALL=C comm -12 "$evidence/removed.txt" "$evidence/ordinary.txt" | wc -l)"
if [ "$removed_ordinary" -ne 0 ]; then
    echo "measure.sh: the timed pass removed $removed_ordinary entries the ordinary build owns ; compare $evidence/removed.txt with $evidence/ordinary.txt before retrying" >&2
    exit 1
fi
if [ ! -s "$evidence/removed.txt" ]; then
    echo "measure.sh: the timed pass removed none of the alternative build's entries; inspect $evidence/timed-pass.log and the backdated entries in $evidence/alternative.txt" >&2
    exit 1
fi
echo "measure.sh: removed $(wc -l <"$evidence/removed.txt") of the alternative build's $(wc -l <"$evidence/alternative.txt") entries; all $(wc -l <"$evidence/ordinary.txt") ordinary entries ($(wc -l <"$evidence/ordinary-hashes-before.txt") unit hashes) remain; lists in $evidence" >&2

if [ -n "$result" ]; then
    if [ ! -s "$result" ]; then
        echo "measure.sh: cargo-prune-superseded wrote no onebudgetspec result to $result; run just bootstrap to restore the pinned reporting tool, then retry" >&2
        exit 1
    fi
    node -e '
        const fs = require("node:fs");
        const [path, seconds] = process.argv.slice(1);
        const report = JSON.parse(fs.readFileSync(path, "utf8"));
        const note = "untimed backlog pass beforehand: " + seconds + " s (not in the figure)";
        report.detail = report.detail ? report.detail + "; " + note : note;
        fs.writeFileSync(path, JSON.stringify(report));
    ' "$result" "$backlog_seconds"
fi
