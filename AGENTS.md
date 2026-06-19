# AGENTS

Durable constraints for this repo. Keep this and every nested `AGENTS.md`
platform-neutral, minimal, and limited to non-obvious, enforceable rules. Record
new durable constraints in the nearest applicable `AGENTS.md` — not in code
comments, commit messages, or one-off notes.

## What this is

`screencomp` — a CLI for the visual-docs framework with deterministic,
network-free operations over a *capture*: a `captures.json` index (each shot's
`name`, `toggles`, content `hash`, and `image` path) plus the PNGs it references,
optionally scoped under a `<root>/<arch>/` layer. Commands: `classify`, `gallery`
(renders user-defined toggle controls — theme, viewport, … — so one screen is one
card you toggle through), `comment`, `manifest` (an image-free digest baseline),
`verify` (the reproducibility gate — two captures of one build must be
byte-identical), `doctor` (preflight the arch subtree and the index), and `arches`
(print the configured `[capture].arches` for the CI matrix). Shots are compared by
the `hash` recorded in `captures.json` — the hash IS the source of truth and
nothing decodes (or re-hashes) images; the capture step owns producing it.

A shot's identity is its `name` plus its toggle map; there is no fixed `project`
dimension — "screen size" and the like are just toggles, declared once in
`screencomp.toml` as `[[toggle]]` tables (`key`, optional `label`, ordered
`values`). The gallery renders one control group per dimension that distinguishes
a name's shots.

Captures always run in a Linux container, so the OS never varies between a
developer and CI — the only dimension that affects pixels is the CPU arch. The
supported arches are declared once in `screencomp.toml` under `[capture].arches`
(e.g. `["arm64"]` or `["x86_64", "arm64"]`); local commands default to the host
arch and CI fans out one capture lane per arch. Native macOS/Windows captures are
not supported (they could never be byte-reproducible between local and CI). Arch
subtree keys are bare arches: `x86_64`, `arm64`; the per-arch baseline is
`shots/baseline/<arch>.json`.

## Two standing goals on every task

The user drives product features and their request is the priority — but carry
two goals into *every* task. When either is the lowest-error path to what the
user asked, fold it into the same task without asking first; surface the rest as
follow-ups.

1. **Engineer the context for next time.** Make the next agent (and you) see
   more for less: realistic e2e tests that exercise what a user actually runs —
   especially when they report a bug existing tests missed, and remembering the
   suite never drives a real browser, so capture/output regressions only surface
   against the [`screencomp-demo`](https://github.com/nickderobertis/screencomp-demo)
   consumer — scripts and `just` recipes that automate repetitive steps and
   shrink their output to signal, and terse `AGENTS.md` notes capturing what the
   code doesn't make obvious.
2. **Engineer the codebase and environment.** Be the engineer the user isn't:
   prioritize the technical initiatives that keep the codebase clean,
   maintainable, and repeatable, and keep setup automated and consistent
   (`just bootstrap` from a clean clone). The strict `just check` gate plus
   local/CI parity (same gate, same pinned `rust-toolchain.toml`) make results
   repeatable. A clean base and a reproducible environment are usually how the
   user's feature ships with a low error rate.

## Stack and composition

This repo is composed from the create-repo skill's reference pieces:

- **Product shape — CLI** (`shapes/cli.md`). A compiled, installable command-line
  tool: the e2e suite drives the real binary as a subprocess and asserts on exit
  codes, stdout/stderr separation, and file effects; the one asset-naming
  contract is shared across every install surface (GitHub Releases,
  `scripts/install.sh`, the composite `action.yml`, and the GHCR image).
- **Language — Rust** (`languages/rust.md`). Pinned stable toolchain in
  `rust-toolchain.toml`; `rustfmt` and `clippy` run as strict deny-warnings
  gates; `cargo nextest` runs unit, integration, and the binary e2e suite;
  `cargo llvm-cov` enforces coverage in the gate; `cargo deny` + `cargo machete`
  are the supply-chain gate. MSRV is declared (`rust-version`) and checked via
  `just msrv`.
- **Cross-cutting — CI** (`ci.md`, always pulled in). Every CI run starts from a
  clean checkout, runs `just bootstrap`, then the `just check` gate across the
  Linux/macOS/Windows matrix that matches the shipped binary; separate jobs prove
  the end-user install path (the composite action in both source and download
  modes) and lint the workflows/Dockerfile. The informational benchmark tier
  lives in its own workflow and never gates.

Composed across two axes — the CLI shape plus the Rust language, with `ci.md` on
top. **Excluded:** `monorepo.md` — this is a single deliverable (one binary
crate, no second app or package or language), so the cross-cutting monorepo
guidance does not apply. No shape↔language intersection reference exists for
Rust CLIs yet, so the two single-axis references are used directly (snapshot- and
asset-naming concerns that such a reference would cover are handled in the e2e
suite and the release workflow).

**Coverage decision.** The gate enforces 95% line coverage
(`cargo llvm-cov --fail-under-lines 95` in `just check`), the skill's default
bar, measured on every PR rather than tracked as a badge. Lower it only with a
documented reason here.

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
- The suite never runs a real browser, so it cannot catch capture-pipeline or
  output-contract regressions that only surface with real screenshots. Validate
  any change to `classify`/`gallery`/`comment`/`manifest` output or the
  `--arch` layout end-to-end against the
  [`screencomp-demo`](https://github.com/nickderobertis/screencomp-demo) consumer
  before release: run its pinned-container capture locally and let its CI exercise
  a visual and a non-visual change. Keep that repo's workflow/example in lockstep
  with this CLI's flags and the documented layout.

## Quality gate

`just check` (aliased as `just full-check`) runs fmt → typecheck → lint → tests
→ e2e → coverage → deps → unused → security → doc → release build → publish
dry-run, stopping at the first failure. It is the single gate CI runs after
`just bootstrap`, mirrored across Linux/macOS/Windows as a hard pass/fail gate.
`check` is the skill-standard verb; `typecheck` is the bare `cargo check`
type-check phase, and `lint`/`format` are the skill-standard names for the strict
clippy and `rustfmt` recipes (`clippy`/`fmt` remain as short aliases).

The performance suite (`benches/`, the `bench*`/`profile` recipes, the perf CI
job) is informational and stays out of `full-check`: its timings are
non-deterministic on shared hardware, so it reports rather than gates. `cargo
check`/`clippy` still cover `benches/` via `--all-targets` so it cannot rot, and
`harness = false` keeps it out of the test runner and coverage.

## Release & git

- Releases are tag-driven (`vX.Y.Z`); the workflow builds per-platform archives
  with sha256 checksums and a multi-arch image, and never publishes untested
  artifacts. crates.io publish is a separately gated step.
- The CLI ships through several surfaces that must stay consistent: release
  binaries, the `scripts/install.sh` installer, crates.io, the GHCR image
  (`Dockerfile`), and the composite actions (`action.yml` install,
  `visual-docs/action.yml` report, `gh-pages-maintenance/action.yml` upkeep). Both
  `install.sh` and the install action download release assets by name, so their
  constructed archive and `.sha256` names must match the release workflow's
  `archive` pattern; change the pattern and you change all three. `examples/` shows
  the intended consumer flow and is excluded from the published crate.
- The reusable workflow is thin glue that `uses:` those composable actions via the
  floating major tag `@v0` (a `uses:` ref can't be interpolated, and an exact pin
  would go stale every release and couldn't reference a brand-new action before it
  ships). `release.yml`'s `advance-major-tag` job force-moves `v0` to each release,
  so `@v0` always resolves to the latest 0.x. An integration test
  (`reusable_workflow_floats_its_own_action_pins`) guards against a regression back
  to exact pins. Bump the floated tag to `v1` only at the 1.0 release.
- The visual-docs surfaces default to the **strict gate** and must stay
  consistent on it: the reusable workflow and the composite action default
  `fail-on-drift: true` + `update-manifest: false`, and `init` scaffolds a matching
  caller plus a `.githooks/pre-push` guard. CI fails on unexpected drift; the
  developer owns the baseline. gh-pages bounding follows the same default-on,
  opt-out shape: the reusable workflow defaults `gh-pages-maintenance: true`
  (gating the `cleanup-preview`/`prune-history` jobs) and `init` forwards the
  `pull_request: closed` + `schedule:` triggers they need — a reusable workflow
  can't self-trigger those, so the caller must forward them. The
  `init`↔reusable-workflow interface is guarded by an integration test — keep the
  inputs in lockstep.
- Release-gating is by commit subject: `release-plz.toml`'s `release_commits`
  releases only `feat`/`fix`/`perf` (or any `type!:` breaking) commits, so a
  squash-merge subject without one of those prefixes ships nothing. Title PRs
  accordingly.
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
