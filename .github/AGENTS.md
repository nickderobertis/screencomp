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
- The downstream half lives in composable actions, not inline workflow steps:
  `action.yml` (install), `visual-docs/action.yml` (gate/gallery/Pages/comment),
  `gh-pages-maintenance/action.yml` (cleanup/prune). The reusable workflow is thin
  glue that `uses:` them, and a hand-rolled caller composes the SAME actions
  (`examples/visual-docs-custom.yml`), so the two paths can't diverge. Because
  `uses:` can't interpolate a ref, the reusable workflow (and the examples)
  reference these actions via the floating major tag `@v0`; `release.yml`'s
  `advance-major-tag` job force-moves `v0` to each release, so `@v0` always
  resolves to the latest 0.x (and a brand-new action becomes referenceable the
  moment it ships). `reusable_workflow_floats_its_own_action_pins` guards against a
  regression to exact pins.
- The gh-pages cleanup/prune logic is `scripts/visual-docs-gh-pages.sh` — the one
  source of truth. The `gh-pages-maintenance` action runs it via
  `$GITHUB_ACTION_PATH` (the script ships beside the action), so the shipped and
  tested logic cannot diverge. The script is shellcheck-linted in CI since
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
- `screencomp-demo`'s entire source is managed HERE, in the `demo/` subdir (the
  source of truth): its static pages, the Playwright config + spec that capture
  them, `screencomp.toml`, and the caller workflow under `demo/.github/`. A CLI
  interface change OR a demo-capture break is fixed by editing `demo/`;
  `sync-demo.yml` then `rsync --delete` mirrors `demo/` directly onto
  `screencomp-demo`'s `main` (no PR — a fully automated loop), preserving the
  demo's `.git`, its committed `shots/baseline/<arch>.sha256` (digests of its real
  pixels, regenerated there), and `.githooks/` (the hook is installed from the
  shared `examples/pre-push`); this internal `demo/README.md` is not pushed. It
  uses the `SCREENCOMP_DEMO_PAT` (`contents: write`; the demo's `main` must allow
  the bot's push), no-ops without the secret, and shares a static `sync-demo`
  concurrency group. Keep `demo/` deterministic and in lockstep with the CLI — a
  stale or non-deterministic managed file ships a broken consumer (an e2e guard,
  `demo_managed_config_is_valid_under_current_schema`, checks `demo/screencomp.toml`
  parses under the live schema). The push triggers the demo's `Visual docs` run on
  the just-synced workflow definition, and the `verify-demo` job waits for that
  exact commit's run, mirrors its pass/fail back into this repo's Actions, and on
  failure dumps the demo's failing logs here — so whether the consumer actually
  works is fully visible from screencomp's CI.
