# Contributing

## Setup

One command provisions a fresh machine end to end:

```sh
./scripts/setup.sh    # or `just setup` once `just` is on PATH
```

It is idempotent and safe to re-run. It installs **asdf** + **direnv**, the
asdf-pinned `just` (`.tool-versions`), the Rust toolchain that `rust-toolchain.toml`
pins (via `rustup`), the cargo dev tools and git hooks (`just bootstrap`), then
allows the `.envrc` and records a stamp under `.dev/` so subsequent checks are
instant. Open a new shell (or `direnv reload`) afterward so asdf/direnv take
effect. Check readiness anytime with `just setup-check`.

Working in **Claude Code**? A `SessionStart` hook (`scripts/session-setup.sh`,
wired in `.claude/settings.json`) runs `setup-check` automatically: if the
environment is ready it stays silent, otherwise it advises running `just setup`
as the first step. Export `SCREENCOMP_AUTO_SETUP=1` to have the hook provision in
a detached background process instead (still non-blocking); `SCREENCOMP_SKIP_SETUP=1`
disables it entirely.

In a remote or CI environment that builds containers from a provisioning step,
run `scripts/setup.sh` there so the toolchain is ready before a session starts:
that avoids the in-session wait and the bootstrap order problem where `just` is
itself installed by setup. Non-interactive shells do not source the login rc, so
the asdf shims and `~/.cargo/bin` must be on `PATH` for tool calls — the setup
scripts and session hook normalise this through `_load_tool_env`, but a bare
shell needs them added explicitly. `lefthook` (git hooks) is the one optional
tool: it has no cargo source fallback, so where its prebuilt is unreachable
`setup-check` reports it as an advisory rather than failing, since building and
testing do not depend on it.

Prefer to wire things by hand? The equivalent manual steps:

1. Install Rust via [rustup](https://rustup.rs). The toolchain, components, and
   release targets are pinned in `rust-toolchain.toml`.
2. Confirm the toolchain: `rustup show`.
3. Install developer tooling and git hooks: `just bootstrap`.
   - Installs `cargo-nextest`, `cargo-llvm-cov`, `cargo-deny`, `cargo-machete`
     (via `cargo-binstall` when present, else `cargo install --locked`) and the
     pinned `lefthook` binary, then installs the hooks.
4. Verify everything: `just full-check`.

## The quality gate

`just full-check` runs every check in order and stops at the first failure:
formatting, `cargo check`, `clippy -D warnings`, unit/integration tests,
end-to-end tests, coverage threshold, dependency/license/source policy,
unused-dependency check, security advisories, docs (`-D` rustdoc warnings),
release build, and the publish dry-run.

Run individual phases while iterating:

```sh
just fmt-check     just clippy        just test
just test-e2e      just test-cov      just deps-check
just security      just doc           just dist-plan
```

## Performance

`screencomp` ships an **informational** performance suite — it measures, it does
not gate, so it is not part of `just full-check`:

- `just bench` — Criterion micro-benchmarks of the in-process pipeline (tree
  walk + SHA-256 + classify/gallery/comment render) at two tree scales; driven
  through the public `run` entrypoint so they track what the binary does.
- `just bench-cli` — end-to-end CLI latency for every verb via hyperfine (real
  process startup + walk + hash + render), writing `target/bench/results.*`.
- `just bench-compare` — diff the latest `bench` run against a `base` baseline
  saved earlier with `just bench-base` (e.g. on `main`, before a change).
- `just profile [...]` — record a sampling profile with samply to find hot
  spots: the in-process pipeline by default (`engine` mode), or a looped real
  CLI invocation (`just profile cli classify`).

`just bench-tools` installs the tools (hyperfine, critcmp, samply); they are
optional and not required by the quality gate. On every pull request, CI runs
the same suite on a fixed runner and posts the numbers as a sticky comment and a
job summary; once the bench lands on `main`, that comment also shows the
regression delta versus the base. Because the timings are noisy, the job reports
rather than blocks — do not add it to required checks.

## House rules

- **No warning backlogs.** Every diagnostic is an error or is disabled — never a
  tolerated warning. Do not introduce `#[allow(...)]` to paper over a lint
  without a comment justifying it. Do not add lint baselines.
- **Quiet on success, useful on failure.** A passing `just` recipe prints
  minimal output. A failing one must surface the command, file/line, rule, and
  diff/exit-code needed to fix it. Keep noisy inspection in `just doctor` and
  friends — never in `full-check`.
- **Real E2E tests.** Any change to user-visible behavior (commands, flags, exit
  codes, output contracts, file effects) needs an end-to-end test in
  `tests/e2e.rs` that drives the compiled binary and asserts the journey. A
  smoke test alone is not enough.
- **Keep `main` thin.** Behavior lives in the library; `src/main.rs` only parses
  args, calls `run`, and maps results to exit codes.
- **Typed errors at the edges, pure domain in the middle.** See `AGENTS.md`.

## Dependency upgrades

`just upgrade` runs `cargo update` and then the full gate. Review the resulting
`Cargo.lock` diff. Upgrades that change behavior, MSRV, or tool versions must
keep `just full-check` green before merging. Do not mutate dependencies outside
this flow without explicit review.

## Commit messages & releases

Commits on `main` follow [Conventional Commits](https://www.conventionalcommits.org):
`feat:` (minor), `fix:`/`perf:` (patch), and `!`/`BREAKING CHANGE:` (major);
`docs`/`test`/`chore`/`ci`/`build`/`style` do not trigger a release. release-plz
reads these to compute the next version, update `CHANGELOG.md`, and open a release
PR that auto-merges once CI is green — see **Releasing** in `README.md`. Do not
bump the version or edit the changelog by hand.

## Agent / editor permissions

`.claude/settings.json` defines a narrow command allowlist (no deny list) so
automated assistants can run the safe `just` quality-gate recipes and a few
repo-scoped direct tool invocations (Cargo, rustup, nextest, llvm-cov,
cargo-deny, cargo-machete, lefthook, and git state commands) without prompting.
It intentionally excludes broad shells (`Bash(*)`, `cargo *`, `git *`, etc.).
Dependency-mutating operations are gated behind `just upgrade`. Keep the file
repo-specific: no personal paths, secrets, or preferences.
