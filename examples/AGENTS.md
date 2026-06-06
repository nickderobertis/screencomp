# AGENTS — examples

- These are copy-paste templates for *consuming* repositories, not part of the
  crate or its test suite; they are excluded from the published package and not
  executed by this repo's CI.
- Keep them minimal and in sync with the CLI's real flags and the documented
  `<root>/<project>/<name>.png` layout — a broken example is worse than none.
- Reference released surfaces only: the `vN` action tag, `ghcr.io/.../screencomp`,
  and `cargo install screencomp`. Do not depend on unreleased behavior.
