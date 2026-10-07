# AGENTS — tools/workspace (the `workspace` project)

- Every gate recipe reaches Nx through `nx` here, which heals the locked install
  first; never call Nx another way from a recipe or workflow.
- Every Nx run takes its environment from `nx-env.mjs` (the wrapper exports it;
  scripts and tests call `runNx`). Most runs are nested inside another Nx task,
  where isolated plugin workers miss Nx's fixed 10s load deadline on a loaded
  host, so plugins load in-process; `tests/nx-env.test.mjs` holds every entry
  point to it.
- Scripts use Node built-ins only (plus, in tests, `yaml` and the PR-title
  action installed from the tag `pr-title.yml` uses, a pairing a test holds);
  Nx is never their runtime dependency except to read the graph. `bun.lock`
  pins the commit that tag resolved to.
- `ci-tier.mjs` reads release-plz's branch prefix from `release-plz.yml`; keep
  the two in step, and fail closed into the sweep when a base cannot be derived.
