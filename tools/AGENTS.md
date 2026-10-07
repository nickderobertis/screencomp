# AGENTS — tools (the Nx project graph)

- Projects: `screencomp` (`src/project.json`), `screencomp-e2e` (`e2e/`),
  `browser-tests`, and, defined here because their files must stay where
  consumers and tools find them, `visual-docs-actions`, `docker-image`, `demo`
  and `workspace`.
- No project is rooted at the repository root: it would own every root file, the
  composite actions among them, so a change to an action would select the crate.
  A file outside every project root reaches its project only through a
  `{workspaceRoot}/…` entry in that project's target `inputs` (Nx's affected
  detection matches changed files against them). Attach any new root-level file
  that way, and never list an action, the reusable workflow or a
  `visual-docs-*.sh` script in another project's inputs.
- Target bodies are `just _…` recipes (Windows runs project commands under
  cmd.exe, so no shell syntax in `project.json`). Target names mean the same in
  every project; `browser-test` (real Chromium) is never a gate target.
- Every project carries exactly one `type:` tag; nx.json's `boundaries.allow`
  says which types each may depend on, and every project's `lint` runs
  `workspace/check-project-boundaries.mjs` for itself. Only `workspace` (the
  coverage aggregate) may depend on `screencomp-e2e`; nothing on `browser-tests`.
- Coverage is combined, never per crate: both Rust `test` targets run `cargo
  llvm-cov --no-report` into `target/llvm-cov-target` after
  `workspace:coverage-clean`, and `workspace:coverage` enforces 95% over the merge.
- `workspace/tests/` (`node --test`, the `workspace:test` target) pins the graph's
  behaviour: affected selection, boundaries, CI tier selection, the workflow
  contracts and the SessionStart hook. Update them with any graph change.
