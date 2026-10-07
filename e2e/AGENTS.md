# AGENTS — e2e (the `screencomp-e2e` project)

- The binary-spawning suite: every user-visible change needs a journey here
  (exit code, stdout/stderr separation, file effects, output contracts).
- A `publish = false` workspace member with no dependency on the crate: it drives
  only the binary, which its `test` target builds beside it (`cargo llvm-cov
  nextest -p screencomp-e2e -p screencomp`) after `screencomp:build`. Resolve the
  binary with `Command::cargo_bin("screencomp")`; `CARGO_PKG_VERSION` here names
  this crate, not screencomp.
- Shares `../tests/common` (via `#[path]`) and `../tests/fixtures` with the
  crate's suites; spawn every subprocess through `common::command`.
- A journey that reads an action, the reusable workflow or a `visual-docs-*.sh`
  script belongs in `tests/actions.rs`, not here: this project must not depend on
  `visual-docs-actions`.
