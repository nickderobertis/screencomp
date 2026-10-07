# AGENTS — tools (the Nx project graph)

- A project whose files must stay where consumers and tools find them is
  defined under `tools/<project>/`, never beside those files.
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
- Every gate target passes on all three `check (<os>)` legs. A check whose
  verdict cannot differ by host and whose tool is Linux-only (actionlint,
  shellcheck, hadolint) is an `[linux]` recipe with a `[macos]`/`[windows]`
  twin that says where it runs; a test whose stand-ins need a POSIX PATH skips
  on Windows through `support.mjs`'s `posixOnly`.
- Every project carries exactly one `type:` tag; nx.json's `boundaries.allow`
  says which types each may depend on, and every project's `lint` runs
  `workspace/check-project-boundaries.mjs` for itself. Only `coverage` may depend
  on `screencomp-e2e`; nothing may depend on `browser-tests` or `coverage`.
- Coverage is combined, never per crate: both Rust `test` targets run `cargo
  llvm-cov --no-report` into `target/llvm-cov-target` after
  `coverage:coverage-clean`, and `coverage:coverage` enforces 95% over the
  merge. It is its own project so that tooling changes never select it.
- `workspace/tests/` (`node --test`, the `workspace:test` target) pins the graph's
  behaviour: affected selection, boundaries, CI tier selection, the workflow
  contracts and the SessionStart hook. Update them with any graph change.
