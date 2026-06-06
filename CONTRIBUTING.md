# Contributing

## Setup

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
