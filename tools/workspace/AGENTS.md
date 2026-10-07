# AGENTS — tools/workspace (the `workspace` project)

- Owns the repository-level targets (`coverage-clean`, `coverage`,
  `supply-chain`, `release-check`, `msrv`) and the gate's tooling: `nx` (the one
  entry point to Nx; quiet on success), `node-modules.sh`,
  `check-project-boundaries.mjs`, `ci-tier.mjs`.
- Scripts use Node built-ins only (plus `yaml` in tests); Nx is never their
  runtime dependency except to read the graph.
- `ci-tier.mjs` reads release-plz's branch prefix from `release-plz.yml`; keep
  the two in step, and fail closed into the sweep when a base cannot be derived.
