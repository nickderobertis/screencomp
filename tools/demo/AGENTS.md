# AGENTS — tools/demo

- `demo/` is mirrored verbatim onto `screencomp-demo`, so nothing repo-internal
  (no project.json, no AGENTS.md) may be added there; this directory defines its
  project instead.
- `demo/` keeps its own npm `package-lock.json` (the demo repository installs with
  npm) and is not a bun workspace member.
