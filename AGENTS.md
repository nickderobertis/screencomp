# AGENTS

Durable constraints for this repo. Keep this and every nested `AGENTS.md`
platform-neutral, minimal, and limited to non-obvious, enforceable rules. Record
new durable constraints in the nearest applicable `AGENTS.md` — not in code
comments, commit messages, or one-off notes.

## What this is

`screencomp` — a CLI for the visual-docs framework with three deterministic,
network-free operations over screenshot trees laid out as
`<root>/<project>/<name>.png`: `classify`, `gallery`, `comment`. Screenshots are
compared by byte digest; nothing decodes images.

## Layout

- `src/main.rs` — thin: parse args, call `run`, map the result to an exit code,
  print errors to stderr. No logic.
- `src/lib.rs` — `run(Cli, &mut dyn Write) -> Result<i32, AppError>`, the only
  orchestration entrypoint.
- `src/cli.rs` — the entire CLI surface (clap derive) in one place.
- `src/commands/` — orchestrate `io` + `domain`, produce output.
- `src/domain/` — pure logic, no I/O.
- `src/io/` — all filesystem access.
- `src/config.rs` / `src/errors.rs` — config loading; typed errors + exit codes.
- `tests/` — in-process integration and binary-spawning e2e suites.

## Toolchain & dependencies

- The toolchain is pinned in `rust-toolchain.toml`; keep `rust-version` in
  `Cargo.toml` in sync. Cargo is the source of truth; `Cargo.lock` is committed.
- Add dependencies only with a concrete need; keep features minimal. Mutate
  dependencies only through `just upgrade`, then re-run the gate.
- No async runtime, network client, or image codec — none are needed.

## Architecture rules

- Keep pure domain logic independent of terminal, filesystem, network, and
  process state. I/O happens only at `src/io` boundaries; never hide it in
  helpers that look pure.
- Errors are typed (`thiserror`); `AppError` maps to exit codes as a tested
  contract. Do not introduce `anyhow`/`miette`.
- Validate inputs (paths, config) at the boundary and return typed errors.
- `pub(crate)` by default. The public API is exactly `run`, `Cli` (and its arg
  tree), `AppError`, and `ConfigError`; widen it only deliberately and document
  it. Prefer narrow, named modules over `utils`; avoid speculative abstraction
  and trait objects without a real boundary.

## Output & exit codes

- stdout carries user/machine output; stderr carries errors (printed in `main`).
  `--format json` is a stable single-line contract; `--quiet` suppresses human
  output but never machine output.
- Exit codes (stable): `0` ok; `1` runtime/IO/config error; `2` CLI usage
  (clap); `3` `classify --exit-code` with differences.

## Diagnostics policy

- No check passes with warnings — every rule is an error or disabled, nothing in
  between. clippy and rustdoc run with `-D warnings`; formatting and coverage
  misses fail their commands. Keep aspirational checks disabled until they can be
  enforced as errors; never accumulate a warning backlog or lint baseline.
- Successful `just` recipes print minimal output; failures preserve the command,
  path/line, rule/diagnostic, and diff/exit-code needed to debug. Noisy
  inspection belongs in dedicated recipes (`just doctor`), never in `full-check`.

## Testing

- Unit tests cover pure domain logic; integration tests drive `run` in-process;
  e2e tests execute the compiled binary and assert real user journeys (exit
  code, stdout, stderr, file effects, output contracts) — not just startup.
  Every user-visible change needs an e2e test; smoke tests are only a subset.
- Tests are deterministic, tempdir-isolated, and offline. Coverage has an
  enforced threshold; do not lower it to pass.

## Quality gate

`just full-check` runs fmt → check → clippy → tests → e2e → coverage → deps →
unused → security → doc → release build → publish dry-run, stopping at the first
failure. CI mirrors this across Linux/macOS/Windows and stays a hard pass/fail
gate.

The performance suite (`benches/`, the `bench*`/`profile` recipes, the perf CI
job) is informational and stays out of `full-check`: its timings are
non-deterministic on shared hardware, so it reports rather than gates. `cargo
check`/`clippy` still cover `benches/` via `--all-targets` so it cannot rot, and
`harness = false` keeps it out of the test runner and coverage.

## Release & git

- Releases are tag-driven (`vX.Y.Z`); the workflow builds per-platform archives
  with sha256 checksums and a multi-arch image, and never publishes untested
  artifacts. crates.io publish is a separately gated step.
- The CLI ships through four surfaces that must stay consistent: release
  binaries, crates.io, the GHCR image (`Dockerfile`), and the composite action
  (`action.yml`). The action's downloaded asset names must match the release
  workflow's `archive` pattern; `examples/` shows the intended consumer flow and
  is excluded from the published crate.
- Manage git state end-to-end; branch off the default branch for changes. Do not
  commit or push unless asked.

## Docs & comments

- Write for the future reader, not as a session log; avoid "added/we
  decided/during setup". Explain surprising constraints, not obvious code. No
  motivational filler.

## Nested AGENTS.md

Subtrees carry their own `AGENTS.md` with only local constraints — never a copy
of this file. A sibling mirror file in each such directory is a symlink to its
`AGENTS.md`; that `AGENTS.md` is the single source of truth, and the mirror must
never hold independent content.
