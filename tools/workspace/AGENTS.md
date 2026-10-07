# AGENTS — tools/workspace (the `workspace` project)

- Every gate recipe reaches Nx through `nx` here, which heals the locked install
  first; never call Nx another way from a recipe or workflow.
- Scripts use Node built-ins only (plus `yaml` and the PR-title action's
  parser in tests, pinned to the versions its tag bundles: bump them with the
  action); Nx is never their runtime dependency except to read the graph.
- `ci-tier.mjs` reads release-plz's branch prefix from `release-plz.yml`; keep
  the two in step, and fail closed into the sweep when a base cannot be derived.
