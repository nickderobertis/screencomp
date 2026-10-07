# AGENTS — tools/visual-docs-actions

- The project definition only: the composite actions, the reusable workflow,
  their smoke-test workflows and `scripts/visual-docs-*.sh` never move (consumers
  reference them by path at `@v0`). They, `README.md` and `examples/` are this
  project's `actions` inputs.
- A test that reads one of those files goes in `tests/actions.rs`, which is this
  project's `test`; nothing else reruns when only an action changes.
