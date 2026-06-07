# Cross-platform verification

The `--platform` mechanism and the single-container standard (added in
[#23](https://github.com/nickderobertis/screencomp/pull/23)) reach environments
this project's CI cannot reproduce: real browser captures, an emulated
`linux/amd64` userland on Apple Silicon, and a full consumer workflow run. This
note records what has been verified and the steps still required to fully trust
the standard configuration in a consuming repository.

The screencomp CLI itself is covered by the offline test suite. These steps
verify the *capture-and-compare pipeline around it*, which depends on a real
renderer and is therefore out of scope for the deterministic, network-free
suite — run them by hand (or in a consumer repo's CI) when adopting the
framework.

## Verified

Exercised locally on `linux-x86_64` with real Playwright (Chromium) screenshots:

- Capture → `classify --platform auto` resolves the host key and reports a match.
- **Same-machine reproducibility**: two independent captures of an unchanged page
  produce byte-identical PNGs when launched with the deterministic flags below.
- Change detection (`changed`, exit `3`), cross-platform isolation (a differing
  `macos-arm64` subtree is ignored when scoped to `linux-x86_64`, and vice
  versa), and the missing-subtree error (exit `1`, names the scoped path).
- `comment --platform … --marker …` emits the per-platform marker/title;
  `gallery --baseline --platform …` scopes both trees and copies the real images
  byte-for-byte.

## Remaining steps

### 1. Capture inside the pinned container (cross-machine reproducibility)

The local check above proves *same-machine* reproducibility only. The standard
relies on the pinned container to make captures identical *across* machines.
Confirm two runs in the container agree, byte for byte:

```sh
IMG=mcr.microsoft.com/playwright:v1.55.0-jammy   # match your pinned Playwright
run() { docker run --rm --platform=linux/amd64 -v "$PWD:/work" -w /work "$IMG" \
          bash -lc "npm ci && OUT=$1 npx playwright test"; }

run shots/baseline/linux-x86_64
run shots/current/linux-x86_64
screencomp classify --baseline shots/baseline --current shots/current \
    --platform linux-x86_64 --exit-code   # expect exit 0
```

### 2. Apple Silicon: emulated amd64 matches native amd64

On an M-series Mac, against a commit whose committed `linux-x86_64` baseline is
already up to date (produced by native-amd64 CI), capture in the emulated
container and compare:

```sh
docker run --rm --platform=linux/amd64 -v "$PWD:/work" -w /work \
  mcr.microsoft.com/playwright:v1.55.0-jammy \
  bash -lc 'npm ci && OUT=shots/current/linux-x86_64 npx playwright test'

screencomp classify --baseline shots/baseline --current shots/current \
    --platform linux-x86_64 --exit-code
```

- **exit 0** — emulated capture is byte-identical to CI; the single-key standard
  holds on Apple Silicon.
- **exit 3** — emulation diverges; switch to per-arch baselines (`linux-x86_64`
  from amd64 CI, `linux-arm64` captured on native Apple Silicon), which the same
  `--platform` mechanism already supports.

### 3. End-to-end workflow run in a consumer repo

`examples/visual-docs.yml` is reasoned-correct but has never executed as a
workflow. In a scratch consumer repo: copy it in, enable Pages
(Settings → Pages → GitHub Actions), point the capture step at a real page, and
open a PR. Confirm:

- the **reproducibility gate** step passes (capture twice, identical bytes);
- the gallery deploys to Pages;
- a sticky diff comment is posted (and re-edited on a second push, not
  duplicated);
- on the PR, a `chore: regenerate screenshot baselines` commit appears and its
  PNG diff is reviewable in the Files view.

### 4. Deterministic-rendering flags

Confirm the documented Chromium flags still exist in the pinned Chromium build
and are applied via the Playwright project's `launchOptions.args`:

```
--headless=new
--disable-gpu --disable-gpu-rasterization --disable-partial-raster
--use-gl=angle --use-angle=swiftshader
--disable-skia-runtime-opts          # CPU-independent path: the key flag
--force-color-profile=srgb
--font-render-hinting=none --disable-lcd-text
--hide-scrollbars
```

plus `deviceScaleFactor: 1` and a fixed viewport. `--disable-skia-runtime-opts`
is the one that makes native and emulated amd64 agree; if step 2 returns exit 3,
check it first.
