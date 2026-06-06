# Shared helpers for the performance scripts (bench.sh, profile.sh). Sourced, not
# executed: callers set their own `set -euo pipefail`.

# Generate a deterministic baseline/current screenshot pair under two directories,
# laid out as the tool expects (`<root>/<project>/<name>.png`). `current` mirrors
# `baseline` except for a realistic mix: ~1 shot per project changed, one added
# (only in current), one removed (only in baseline). Content is reproducible —
# no image codec is involved because the tool compares files by byte digest, so
# any stable bytes of the right length exercise the same walk-and-hash path a
# real capture would.
#
# Usage: bench_gen_tree BASELINE_DIR CURRENT_DIR PROJECTS SHOTS KB
bench_gen_tree() {
    local baseline="$1" current="$2" projects="$3" shots="$4" kb="$5"
    local bytes=$((kb * 1024))
    local p s proj seed
    for ((p = 0; p < projects; p++)); do
        proj="$(printf 'project%02d' "$p")"
        mkdir -p "$baseline/$proj" "$current/$proj"
        for ((s = 0; s < shots; s++)); do
            seed=$((p * shots + s))
            _bench_write_shot "$baseline/$proj/$(printf 'shot%02d.png' "$s")" "$seed" "$bytes"
            # Change one shot in eight; the rest stay byte-identical (unchanged).
            if ((s % 8 == 0)); then
                _bench_write_shot "$current/$proj/$(printf 'shot%02d.png' "$s")" "$((seed + 100000))" "$bytes"
            else
                _bench_write_shot "$current/$proj/$(printf 'shot%02d.png' "$s")" "$seed" "$bytes"
            fi
        done
        _bench_write_shot "$current/$proj/added.png" "$((900000 + p))" "$bytes"
        _bench_write_shot "$baseline/$proj/removed.png" "$((800000 + p))" "$bytes"
    done
}

# Write a file of exactly $3 bytes whose content is determined by seed $2, so
# digests differ per seed and match across baseline/current when seeds match.
_bench_write_shot() {
    local path="$1" seed="$2" bytes="$3"
    yes "screencomp-bench-$seed" | head -c "$bytes" >"$path"
}
