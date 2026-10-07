# AGENTS — tests

- `integration.rs` drives `screencomp::run` in-process: parse `Cli`, capture a
  buffer, assert exit code, output, and file effects.
- `actions.rs` holds the contract tests over the shipped visual-docs surfaces
  (composite actions, reusable workflow, `scripts/visual-docs-*.sh`, and every
  documented or synced copy of the capture invocation). Any test that reads one
  of those files belongs here, not in `integration.rs`: this binary is also the
  `visual-docs-actions` project's `test`, which is what reruns it when only an
  action changes.
- The binary-spawning e2e suite is its own crate, `e2e/` (see its AGENTS.md);
  add an e2e case there for every user-visible change.
- `common/mod.rs` is shared by these suites (`mod common;`) and by `e2e/` (via
  `#[path]`); a subdirectory of `tests/` is not a test target, which is what
  makes that work.
- Spawn every subprocess through `common::command`, never
  `std::process::Command::new`. The pre-push hook runs these suites and Git
  exports `GIT_DIR` to a hook, so an inheriting `git` — or a shipped script that
  calls one — acts on the repository being pushed, not on the tempdir.
- `fixtures/` are opaque byte blobs (not rendered PNGs); keep them in sync with
  asserted expectations. Tests stay deterministic, tempdir-isolated, and offline.
