# Examples

Copy-paste starting points for adopting `screencomp` in a consuming repository.
These files are templates for *your* repo — they are excluded from the published
crate and are not run by this project's CI.

## `visual-docs.yml`

An end-to-end GitHub Actions workflow that captures screenshots, builds a gallery,
publishes it to GitHub Pages, and posts a sticky screenshot-diff comment on pull
requests. Copy it into `.github/workflows/` and adapt the **Capture screenshots**
step to your stack (Playwright, Cypress, Storybook, …).

Prerequisites:

- A committed baseline under `shots/baseline/<project>/<name>.png`.
- The capture step writes the current run to `shots/current/<project>/<name>.png`.
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
