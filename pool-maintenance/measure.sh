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
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."

if ! command -v cargo-prune-superseded >/dev/null 2>&1; then
    echo "measure.sh: cargo-prune-superseded is not on PATH" >&2
    echo "ACTION: run 'just bootstrap', which installs the pinned release" >&2
    exit 1
fi
result="${ONEBUDGETSPEC_RESULT:-}"
evidence=target/pool-maintenance-budget
rm -rf "$evidence"
mkdir -p "$evidence"

# Every build entry the pruner selects among: the children of deps/,
# .fingerprint/, build/ and incremental/ in each profile directory (one holding
# .fingerprint/), sorted.
entries() {
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
    just _rust-test screencomp
    just _rust-test screencomp-e2e screencomp
}

now() { date +%s.%N; }

echo "measure.sh: (a) untimed backlog pass" >&2
started="$(now)"
env -u ONEBUDGETSPEC_RESULT just pool-maintenance >&2
backlog_seconds="$(awk -v a="$started" -v b="$(now)" 'BEGIN { printf "%.3f", b - a }')"

echo "measure.sh: (b) gate test build under an alternative profile setting" >&2
entries >"$evidence/before-alternative.txt"
CARGO_PROFILE_DEV_DEBUG=line-tables-only gate_build >&2
entries >"$evidence/after-alternative.txt"
LC_ALL=C comm -13 "$evidence/before-alternative.txt" "$evidence/after-alternative.txt" >"$evidence/alternative.txt"
if [ ! -s "$evidence/alternative.txt" ]; then
    echo "measure.sh: the alternative build created no new build entries, so there is nothing superseded to time" >&2
    exit 1
fi
while IFS= read -r entry; do
    find "$entry" -exec touch -h -d '2000-01-01T00:00:00Z' {} +
done <"$evidence/alternative.txt"

echo "measure.sh: (c) ordinary gate test build" >&2
gate_build >&2
entries >"$evidence/before-pass.txt"
LC_ALL=C comm -23 "$evidence/before-pass.txt" "$evidence/alternative.txt" >"$evidence/ordinary.txt"

echo "measure.sh: (d) timed pass" >&2
just pool-maintenance >&2
entries >"$evidence/after-pass.txt"
LC_ALL=C comm -23 "$evidence/before-pass.txt" "$evidence/after-pass.txt" >"$evidence/removed.txt"
by_hash "$evidence/ordinary.txt" >"$evidence/ordinary-hashes-before.txt"
LC_ALL=C comm -12 "$evidence/ordinary.txt" "$evidence/after-pass.txt" >"$evidence/ordinary-kept.txt"
by_hash "$evidence/ordinary-kept.txt" >"$evidence/ordinary-hashes-after.txt"

removed_ordinary="$(LC_ALL=C comm -12 "$evidence/removed.txt" "$evidence/ordinary.txt" | wc -l)"
if [ "$removed_ordinary" -ne 0 ]; then
    echo "measure.sh: the timed pass removed $removed_ordinary entries the ordinary build owns (see $evidence/removed.txt)" >&2
    exit 1
fi
if [ ! -s "$evidence/removed.txt" ]; then
    echo "measure.sh: the timed pass removed none of the alternative build's entries" >&2
    exit 1
fi
echo "measure.sh: removed $(wc -l <"$evidence/removed.txt") of the alternative build's $(wc -l <"$evidence/alternative.txt") entries; all $(wc -l <"$evidence/ordinary.txt") ordinary entries ($(wc -l <"$evidence/ordinary-hashes-before.txt") unit hashes) remain; lists in $evidence" >&2

if [ -n "$result" ]; then
    if [ ! -s "$result" ]; then
        echo "measure.sh: cargo-prune-superseded wrote no onebudgetspec result to $result" >&2
        exit 1
    fi
    # The `${…}` below is a JavaScript template literal for node, not the shell's.
    # shellcheck disable=SC2016
    node -e '
        const fs = require("node:fs");
        const [path, seconds] = process.argv.slice(1);
        const report = JSON.parse(fs.readFileSync(path, "utf8"));
        const note = `untimed backlog pass beforehand: ${seconds} s (not in the figure)`;
        report.detail = report.detail ? `${report.detail}; ${note}` : note;
        fs.writeFileSync(path, JSON.stringify(report));
    ' "$result" "$backlog_seconds"
fi
