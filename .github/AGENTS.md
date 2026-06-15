# AGENTS — .github

- CI is a hard pass/fail gate: every run does a clean checkout, `just bootstrap`,
  then `just check` (the `check` matrix job) across Linux/macOS/Windows. Never let
  a job pass with warnings.
- Pin actions to a stable major or a commit SHA. Default `permissions` to
  `contents: read`; grant a write scope only on the job that needs it (the binary
  upload job gets `contents: write`, the image push job `packages: write`).
- The release workflow runs tests before building any artifact and never
  publishes untested binaries or images. crates.io publish stays gated behind a
  repo variable and a token secret.
- Provision the toolchain with `rustup show` so `rust-toolchain.toml` stays the
  single source of truth.
- The reusable action, Dockerfile, and example workflow are part of the gate:
  CI lints them (actionlint/hadolint), runs the action against its own checkout,
  and builds the image. Keep `action.yml` asset names in lockstep with the
  release workflow's `archive` pattern.
- The gh-pages cleanup/prune logic is `scripts/visual-docs-gh-pages.sh` — the one
  source of truth. The reusable workflow's `cleanup-preview`/`prune-history` jobs
  fetch it at their own ref (`github.job_workflow_ref`) rather than inlining it, so
  the shipped logic and the tested logic cannot diverge. `lint-actions` and CI
  `-ignore` the `job_workflow_ref` schema-gap message (it is a real context
  property actionlint 1.7.7 lacks); the script is shellcheck-linted in CI since
  actionlint only covers shell embedded in workflows.
- `test-gh-pages-maintenance.yml` exercises that script against a *disposable*
  branch on `screencomp-demo` (never its real gh-pages) via the
  `SCREENCOMP_DEMO_PAT` secret (a token with `contents: write` on the demo repo;
  the default `GITHUB_TOKEN` cannot push cross-repo). It runs on PRs that touch
  the script or the reusable/maintenance workflows (plus manual and weekly), and
  no-ops without the secret, so it never blocks forks or unconfigured clones —
  fork PRs get no secret and skip. Use plain `pull_request`, never
  `pull_request_target`, so untrusted fork code never runs with the secret. A
  single static concurrency group (`test-gh-pages-maintenance`, not keyed by ref,
  `cancel-in-progress: false`) serializes every run so two never mutate the shared
  demo repo at once and an in-flight run always finishes its branch cleanup.
