# AGENTS — tools/workspace (the `workspace` project)

- Every gate recipe reaches Nx through `nx` here, which heals the locked install
  first; never call Nx another way from a recipe or workflow.
- Scripts use Node built-ins only (plus `yaml` in tests); Nx is never their
  runtime dependency except to read the graph.
- `ci-tier.mjs` reads release-plz's branch prefix from `release-plz.yml`; keep
  the two in step, and fail closed into the sweep when a base cannot be derived.
