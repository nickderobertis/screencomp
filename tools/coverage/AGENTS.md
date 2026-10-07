# AGENTS — tools/coverage (the `coverage` project)

- The 95% line floor over the crate's sources, enforced once over the merged
  profiles of `screencomp:test` and `screencomp-e2e:test`; never per crate, and
  never lowered to absorb a split.
- Nothing may depend on it, and it depends only on the suites it merges, so a
  change that reaches neither never pays for it.
