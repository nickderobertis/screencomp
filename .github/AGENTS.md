# AGENTS — .github

- CI is a hard pass/fail gate: every run does a clean checkout, `just bootstrap`,
  then `just check` (the `check` matrix job) across Linux/macOS/Windows. Never let
  a job pass with warnings.
- Pin actions to a stable major or a commit SHA. Default `permissions` to
  `contents: read`; grant a write scope only on the job that needs it (the binary
  upload job gets `contents: write`, the image push job `packages: write`).
- The visual-docs concurrency group (`visual-docs-${{ github.ref }}`, in the
  reusable workflow and both `examples/visual-docs*.yml`) must keep
  `cancel-in-progress` scoped to PRs (`github.event_name == 'pull_request'`), never
  a bare `true`. The default branch's key (`refs/heads/main`) is shared by every
  push and the scheduled prune, so blanket cancellation lets a follow-up push (a
  quick second merge, the sync bot) kill an in-flight canonical gallery deploy ~2s
  in — a half-deployed gallery someone then re-runs by hand. PR previews still
  cancel (supersede stale ones); push/schedule queue on the shared ref.
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
- `test-visual-docs.yml` is the PR-time observability for the CLI's
  consumer-facing output contract (classify/comment/gallery/manifest) — the real
  browser capture (`verify-demo`) only runs post-release, too late to block a
  shipped regression. So it triggers on `src/**` + the Cargo manifests (not just
  the workflow/action files), and its `action-smoke` job builds the CLI from the
  PR (`method: source`) and drives the composite action through BOTH an unchanged
  and a drifted capture. Two non-obvious invariants keep it honest: the seeded
  baseline must be `.json` (the format the action looks for — the obsolete
  `.sha256` makes `has_baseline` false and silently skips classify while still
  going green), and the drift step must keep exercising the changed-shot branch
  (classify exit 3 + the gate/comment render), which the unchanged path never
  touches.
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
  demo's `.git`, its committed `shots/baseline/<arch>.json`, and `.githooks/` (the
  hook is installed from the shared `examples/pre-push`); this internal
  `demo/README.md` is not pushed. The demo's capture is one script,
  `demo/capture.sh`, shared by the caller workflow's `capture-command` and the
  reseed below so they can't diverge. When a screencomp release changes the
  capture index's format and leaves that baseline unreadable, `sync-demo` (which
  also runs `on: release: published`, when the demo's `@v0` CLI is the just-released
  one) reseeds `shots/baseline/<arch>.json` from a fresh capture in the SAME pinned
  Playwright container — the byte-reproducibility the whole project rests on — and
  commits it in the same push, so the demo self-heals from seed-not-gate back to
  gating with no human step. A baseline that merely *drifted* (still readable) is
  left alone — that stays the developer's to own. It
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
