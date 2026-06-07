# Examples

Copy-paste starting points for adopting `screencomp` in a consuming repository.
These files are templates for *your* repo — they are excluded from the published
crate and are not run by this project's CI.

## `visual-docs.yml`

An end-to-end GitHub Actions workflow that captures screenshots, builds a gallery,
publishes it to GitHub Pages, and posts a sticky screenshot-diff comment on pull
requests. Copy it into `.github/workflows/` and adapt the **Capture screenshots**
step to your stack (Playwright, Cypress, Storybook, …).

> **See it running:** [`screencomp-demo`](https://github.com/nickderobertis/screencomp-demo)
> is a live consumer of this standard — real Playwright captures in the pinned
> container, exercised on every pull request (visual and non-visual changes,
> locally and in CI). It is a good template to copy from.

### Image-free baselines (digest manifest)

The workflow commits only a tiny per-platform **digest manifest**
(`shots/baseline/<platform>.sha256`), never the baseline PNGs. Because
`screencomp` compares by content digest, the manifest is all that `classify` and
`comment` need, so the repository never accumulates binary history and grows
without bound. The manifest's diff in a PR (old hash → new hash per shot) is the
precise record of what changed; the gallery published to Pages shows the current
pixels. Seed it once with `screencomp manifest --input shots/current --platform
auto --output shots/baseline/<platform>.sha256` and commit it.

### Standard configuration: capture in one pinned container

A screenshot's bytes depend on the OS, CPU, fonts, and GPU that rendered it. The
standard configuration fixes all of them by capturing **and** comparing inside a
single pinned `linux/amd64` container — on CI and locally — so captures are
byte-reproducible everywhere and there is one platform key, `linux-x86_64`.

macOS cannot run Linux containers natively (Docker runs a Linux VM), so a
container on a Mac renders Linux pixels; `--platform=linux/amd64` makes them the
same `linux/amd64` pixels as CI (emulated via Rosetta/QEMU). This trades
real-macOS rendering fidelity for exact reproducibility. The decisive flag is
`--disable-skia-runtime-opts`, which forces a CPU-independent render path so
emulated and native amd64 match; the workflow also runs a **reproducibility
gate** (capture twice, require identical bytes) so a nondeterministic pipeline
fails loudly. See the comments in `visual-docs.yml` for the full flag list and
the one-time command that validates emulated capture against CI on Apple Silicon.

On Apple Silicon, enable Docker Desktop's **Rosetta** for amd64 emulation. Under
the QEMU fallback, `--use-angle=swiftshader` can crash Chromium
(`qemu: uncaught target signal`); a CPU-rasterization fallback (`--disable-gpu`,
one browser per context, keeping `--disable-skia-runtime-opts`) is byte-identical
to native CI — see `screencomp-demo` for a worked configuration.

Prerequisites:

- A committed baseline manifest at `shots/baseline/linux-x86_64.sha256` (text).
- The capture step writes the current run to
  `shots/current/linux-x86_64/<project>/<name>.png` inside the pinned container.
- GitHub Pages enabled (**Settings → Pages → Build and deployment → GitHub Actions**).

## Installing the CLI

The workflow installs the CLI with the bundled composite action:

```yaml
- uses: nickderobertis/screencomp@v1
  with:
    version: latest # or a pinned tag like v0.1.0
```

Other options, outside Actions:

```sh
# From crates.io
cargo install screencomp

# From a release archive (Linux/macOS/Windows binaries + checksums)
#   https://github.com/nickderobertis/screencomp/releases
```

## Running via the container image

A multi-arch image is published to GitHub Container Registry. Mount your
screenshots and run any subcommand; pass `--user` so files written back to the
host are owned by you:

```sh
docker run --rm \
  --user "$(id -u):$(id -g)" \
  -v "$PWD:/work" \
  ghcr.io/nickderobertis/screencomp:latest \
  gallery --input shots/current --output site --title "Visual docs"
```

The image's entrypoint is `screencomp`, so the trailing arguments are the
subcommand and its flags.
