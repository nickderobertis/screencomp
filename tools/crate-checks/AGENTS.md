# AGENTS — tools/crate-checks (the `crate-checks` project)

- Holds the crate's non-test repository checks (`supply-chain`,
  `release-check`, `msrv`) apart from `workspace`, so the gate tooling's tests
  are never selected by a crate change and these are selected only by one.
  Every check here runs in the sweep; never drop one to speed up a tier.
