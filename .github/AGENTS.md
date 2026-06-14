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
