# Shared helpers for the performance scripts (bench.sh, profile.sh). Sourced, not
# executed: callers set their own `set -euo pipefail`.

# Generate a deterministic baseline/current capture pair under two directories,
# each laid out as the tool expects: a `captures.json` index plus the PNGs it
# references. `current` mirrors `baseline` except for a realistic mix: ~1 shot per
# group changed, one added (only in current), one removed (only in baseline).
# Content is reproducible — no image codec is involved, and the hash recorded in
# the index is seed-derived (the tool trusts the index, never re-hashing), so any
# stable bytes of the right length exercise the same parse-and-render path a real
# capture would.
#
# Usage: bench_gen_tree BASELINE_DIR CURRENT_DIR PROJECTS SHOTS KB
bench_gen_tree() {
    local baseline="$1" current="$2" projects="$3" shots="$4" kb="$5"
    local bytes=$((kb * 1024))
    local p s proj seed image
    local -a base_shots=() cur_shots=()
    for ((p = 0; p < projects; p++)); do
        proj="$(printf 'project%02d' "$p")"
        mkdir -p "$baseline/$proj" "$current/$proj"
        for ((s = 0; s < shots; s++)); do
            seed=$((p * shots + s))
            image="$proj/$(printf 'shot%02d.png' "$s")"
            _bench_write_shot "$baseline/$image" "$seed" "$bytes"
            base_shots+=("$(_bench_shot_json "$(printf '%s-shot%02d' "$proj" "$s")" "$image" "$seed")")
            # Change one shot in eight; the rest stay identical (unchanged).
            if ((s % 8 == 0)); then
                _bench_write_shot "$current/$image" "$((seed + 100000))" "$bytes"
                cur_shots+=("$(_bench_shot_json "$(printf '%s-shot%02d' "$proj" "$s")" "$image" "$((seed + 100000))")")
            else
                _bench_write_shot "$current/$image" "$seed" "$bytes"
                cur_shots+=("$(_bench_shot_json "$(printf '%s-shot%02d' "$proj" "$s")" "$image" "$seed")")
            fi
        done
        # Added only in current; removed only in baseline.
        _bench_write_shot "$current/$proj/added.png" "$((900000 + p))" "$bytes"
        cur_shots+=("$(_bench_shot_json "$(printf '%s-added' "$proj")" "$proj/added.png" "$((900000 + p))")")
        _bench_write_shot "$baseline/$proj/removed.png" "$((800000 + p))" "$bytes"
        base_shots+=("$(_bench_shot_json "$(printf '%s-removed' "$proj")" "$proj/removed.png" "$((800000 + p))")")
    done
    _bench_write_index "$baseline/captures.json" "${base_shots[@]}"
    _bench_write_index "$current/captures.json" "${cur_shots[@]}"
}

# Write a file of exactly $3 bytes whose content is determined by seed $2, so the
# referenced image differs per seed. No pipes: a `producer | head -c` would leave
# the producer killed by SIGPIPE, which trips `set -o pipefail` in the calling
# scripts. A seed-tagged prefix padded with spaces to the target length is
# deterministic and codec-free (the tool never decodes the image).
_bench_write_shot() {
    local path="$1" seed="$2" bytes="$3"
    local head="screencomp-bench-${seed}:"
    local pad=$((bytes - ${#head}))
    ((pad < 0)) && pad=0
    { printf '%s' "$head"; printf '%*s' "$pad" ''; } >"$path"
}

# Emit one `captures.json` shot object: a seed-derived 64-hex digest (equal across
# baseline/current when seeds match, so the classify mix is preserved) and no
# toggles. Args: NAME IMAGE SEED.
_bench_shot_json() {
    local name="$1" image="$2" seed="$3"
    printf '{"name":"%s","toggles":{},"hash":"%064x","image":"%s"}' "$name" "$seed" "$image"
}

# Write a `captures.json` to $1 listing the remaining shot-object arguments.
_bench_write_index() {
    local path="$1"
    shift
    local IFS=','
    printf '{"schema":1,"shots":[%s]}' "$*" >"$path"
}
