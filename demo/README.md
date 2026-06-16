# `demo/` — managed integration files for `screencomp-demo`

This directory is the **source of truth** for the screencomp-integration files in
the [`screencomp-demo`](https://github.com/nickderobertis/screencomp-demo)
consumer repo. Editing them here and merging to `main` triggers
[`.github/workflows/sync-demo.yml`](../.github/workflows/sync-demo.yml), which
commits those files **directly to `screencomp-demo`'s `main`** (a fully automated
loop — no PR to merge), then waits for the demo's resulting `Visual docs` run and
mirrors its pass/fail back into this repo's Actions.

The point is to kill cross-repo drift: when the CLI's interface changes (a new
flag, a renamed config key, a reusable-workflow input), the demo migrates by
editing this subdir — not by a separate hand-migration of another repo that is
easy to forget. The demo caller pins the floating `@v0` tag, so most changes
reach it the moment a release advances `@v0`, and the ones that don't (the files
below) are carried by the sync.

## Managed file manifest

The sync copies these into `screencomp-demo` (src here → dest there):

| Source (this repo)     | Destination (`screencomp-demo`)        |
| ---------------------- | -------------------------------------- |
| `demo/screencomp.toml` | `screencomp.toml`                      |
| `demo/visual-docs.yml` | `.github/workflows/visual-docs.yml`    |
| `examples/pre-push`    | `.githooks/pre-push`                   |

The hook is reused verbatim from `examples/pre-push` (the canonical runtime-arch
hook) so there is one source for it.

## What is NOT managed here

- The demo's **application** and its **capture** (e.g. its Playwright config/tests
  that write `$SHOTS_OUT/<project>/<name>.png`) live in `screencomp-demo`.
- The committed **baselines** (`shots/baseline/<arch>.sha256`) are digests of the
  demo's actual rendered pixels, so they are owned and regenerated there, never
  synced from here.
