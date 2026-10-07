# AGENTS — tools/screencomp-cli (the `screencomp-cli` project)

- The binary as an artifact: the crate's sources and manifest (its overridden
  `default` inputs) and the `build` other projects run it from. Every project
  that builds, runs or ships the CLI depends on this one, never on
  `screencomp`, whose suites are a leaf. Keep tests/ and benches/ out of its
  inputs, or a test-only change would select every dependent again.
