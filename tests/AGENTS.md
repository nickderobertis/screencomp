# AGENTS — tests

- `integration.rs` drives `screencomp::run` in-process: parse `Cli`, capture a
  buffer, assert exit code, output, and file effects.
- `e2e.rs` spawns the compiled binary (`assert_cmd`) and asserts user journeys —
  exit code, stdout/stderr separation, file effects, JSON/Markdown contracts.
- Add an e2e case for every user-visible change; a smoke test alone is not
  enough.
- `fixtures/` are opaque byte blobs (not rendered PNGs); keep them in sync with
  asserted expectations. Tests stay deterministic, tempdir-isolated, and offline.
