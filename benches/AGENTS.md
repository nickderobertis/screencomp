# AGENTS — benches

- Bench through the public entrypoint (`run`), constructing the `Cli` arg tree
  directly so the numbers track what the binary runs and keep clap parsing out of
  the measurement. Do not widen the crate's public API to reach internals.
- Build the synthetic screenshot trees on disk once, outside every timed loop;
  the hot path is the directory walk plus a SHA-256 over every file, so never let
  tree construction leak into a measurement.
- `cargo check`/`clippy` cover this target via `--all-targets`; keep it
  warning-clean so it cannot rot. `harness = false` keeps it out of the test
  runner and coverage.
