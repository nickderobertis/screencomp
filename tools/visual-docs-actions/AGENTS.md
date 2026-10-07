# AGENTS — tools/visual-docs-actions

- The project definition only: the composite actions, the reusable workflow,
  their smoke-test workflows and `scripts/visual-docs-*.sh` never move (consumers
  reference them by path at `@v0`). They, `README.md` and `examples/` are this
  project's `actions` inputs.
- Its `test` is the crate's `tests/actions.rs` binary; its `lint` is actionlint
  and shellcheck over those files.
