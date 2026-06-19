# `demo/` — the managed source of `screencomp-demo`

This directory **is** the source tree of the
[`screencomp-demo`](https://github.com/nickderobertis/screencomp-demo) consumer
repo: its static pages, the Playwright config + spec that capture them, the
`screencomp.toml`, and the caller workflow under `demo/.github/`. Editing it here
and merging to `main` triggers
[`.github/workflows/sync-demo.yml`](../.github/workflows/sync-demo.yml), which
mirrors it **directly onto `screencomp-demo`'s `main`** (a fully automated loop —
no PR to merge), then waits for the demo's resulting `Visual docs` run and mirrors
its pass/fail back into this repo's Actions (`verify-demo`), dumping the failing
logs here when it is red.

The point is to kill cross-repo drift and give one place to fix the consumer: when
the CLI's interface changes (a flag, a config key, a reusable-workflow input) — or
the demo's capture itself breaks — you fix it by editing this subdir, never by a
separate hand-migration of another repo.

## What is mirrored

The whole tree is `rsync`'d onto the demo (`demo/X` → demo's `X`), so the demo's
source equals this directory. Notable pieces:

| Path                                  | Role                                                |
| ------------------------------------- | --------------------------------------------------- |
| `demo/screencomp.toml`                | `[capture].arches` + comment/guard config           |
| `demo/.github/workflows/visual-docs.yml` | caller of the reusable workflow (`@v0`)          |
| `demo/package.json` / `package-lock.json` | pinned `@playwright/test` (matches the CI image) |
| `demo/playwright.config.ts`           | deterministic capture settings                      |
| `demo/tests/screenshots.spec.ts`      | writes `$SHOTS_OUT/captures.json` + the PNGs it references |
| `demo/pages/*.html`                   | the static pages being photographed                 |
| `demo/.gitignore`                     | commit the digest baseline, ignore generated PNGs   |

The pre-push hook is installed from `examples/pre-push` (one source, shared with
the documented consumer template).

## What the sync preserves (never overwrites)

- The demo's **`.git`**, its committed **baselines** (`shots/baseline/<arch>.json`
  — digests of its real pixels, regenerated there), and its `.githooks/` (the hook
  is (re)installed from `examples/pre-push`). This `README.md` is not pushed.

## Editing here

Keep the capture deterministic (static content, the launch flags in
`playwright.config.ts`) so two captures of one build stay byte-identical — that is
what `screencomp verify` enforces. `npm run -s` nothing is needed locally; the
demo's CI (surfaced via `verify-demo`) is the validation.
