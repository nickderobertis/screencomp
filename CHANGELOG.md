# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.4.2](https://github.com/nickderobertis/screencomp/compare/v0.4.1...v0.4.2) - 2026-06-20

### Added

- single top toggle bar that filters the gallery ([#76](https://github.com/nickderobertis/screencomp/pull/76))

## [0.4.1](https://github.com/nickderobertis/screencomp/compare/v0.4.0...v0.4.1) - 2026-06-20

### Added

- surface cross-CPU anti-aliasing drift as a first-class diagnosis ([#72](https://github.com/nickderobertis/screencomp/pull/72))

## [0.4.0](https://github.com/nickderobertis/screencomp/compare/v0.3.1...v0.4.0) - 2026-06-19

### Added

- [**breaking**] user-defined gallery toggles backed by a captures.json index ([#68](https://github.com/nickderobertis/screencomp/pull/68))

### Added

- [**breaking**] user-defined gallery toggles backed by a `captures.json` index.
  A capture is now a directory holding `captures.json` (each shot's `name`,
  `toggles`, content `hash`, and `image` path) plus the PNGs it references, rather
  than a `<project>/<name>.png` tree. A shot's identity is its name plus a toggle
  map — the old fixed `project` dimension is gone; declare toggle dimensions
  (theme, viewport, …) in `screencomp.toml` as `[[toggle]]` tables and the gallery
  renders one control group per dimension so a screen is a single card you toggle
  through. `manifest` writes a JSON baseline (`<arch>.json`, was `<arch>.sha256`);
  the `classify`/`verify` JSON contracts replace `project`+`name` with `name`+a
  `toggles` map; `comment` resolves inline images by each shot's `image` path.
  Migrate by having your capture step emit `captures.json` and re-seeding the
  baseline with `screencomp manifest`.

## [0.3.1](https://github.com/nickderobertis/screencomp/compare/v0.3.0...v0.3.1) - 2026-06-19

### Added

- close scaffold gaps — proxy-ready hook, guard enablement, doctor --env ([#66](https://github.com/nickderobertis/screencomp/pull/66))

## [0.3.0](https://github.com/nickderobertis/screencomp/compare/v0.2.4...v0.3.0) - 2026-06-16

### Added

- [**breaking**] replace screenshot platform key with CPU arch + config-driven [capture].arches ([#58](https://github.com/nickderobertis/screencomp/pull/58))

## [0.2.4](https://github.com/nickderobertis/screencomp/compare/v0.2.3...v0.2.4) - 2026-06-16

### Fixed

- *(init)* default capture key to the host arch on a Linux container ([#55](https://github.com/nickderobertis/screencomp/pull/55))

## [0.2.3](https://github.com/nickderobertis/screencomp/compare/v0.2.2...v0.2.3) - 2026-06-15

### Added

- *(visual-docs)* keep recent gallery versions on gh-pages prune ([#53](https://github.com/nickderobertis/screencomp/pull/53))

## [0.2.2](https://github.com/nickderobertis/screencomp/compare/v0.2.1...v0.2.2) - 2026-06-15

### Changed

- decompose visual-docs into composable actions over a thin workflow ([#50](https://github.com/nickderobertis/screencomp/pull/50))

### Fixed

- float the reusable workflow's internal action refs on @v0 ([#51](https://github.com/nickderobertis/screencomp/pull/51))

## [0.2.1](https://github.com/nickderobertis/screencomp/compare/v0.2.0...v0.2.1) - 2026-06-15

### Added

- bound gh-pages gallery growth with preview cleanup and history prune ([#47](https://github.com/nickderobertis/screencomp/pull/47))

## [0.2.0](https://github.com/nickderobertis/screencomp/compare/v0.1.15...v0.2.0) - 2026-06-14

### Changed

- [**breaking**] *(visual-docs)* default the reusable workflow and composite action to the strict gate: `fail-on-drift: true` (CI fails on unexpected drift) and `update-manifest: false` (the baseline is no longer auto-pushed). Restore CI auto-accept with `fail-on-drift: false` + `update-manifest: true` ([#44](https://github.com/nickderobertis/screencomp/pull/44))

### Added

- *(init)* scaffold the strict-gate turnkey — an executable `.githooks/pre-push` guard plus a `fail-on-drift: true` caller workflow ([#44](https://github.com/nickderobertis/screencomp/pull/44))
- *(action)* `fail-on-drift` and `comment-base-ref` inputs on the reusable workflow and composite action; `comment-base-ref` defaults to the PR base branch so the before/after comment still renders under the strict gate ([#44](https://github.com/nickderobertis/screencomp/pull/44))

## [0.1.15](https://github.com/nickderobertis/screencomp/compare/v0.1.14...v0.1.15) - 2026-06-14

### Added

- *(action)* add visual-docs composite action for custom-capture jobs ([#42](https://github.com/nickderobertis/screencomp/pull/42))

## [0.1.14](https://github.com/nickderobertis/screencomp/compare/v0.1.13...v0.1.14) - 2026-06-14

### Added

- *(config)* auto-discover screencomp.toml by walking up from the cwd ([#39](https://github.com/nickderobertis/screencomp/pull/39))

## [0.1.13](https://github.com/nickderobertis/screencomp/compare/v0.1.12...v0.1.13) - 2026-06-14

### Fixed

- *(ci)* split the reusable visual-docs workflow into a container `capture` job
  and a host `report` job so `gh`/`git`/screencomp run where they exist, fixing
  `gh pr comment` failing inside the capture container; add a self-test workflow
  that exercises the pipeline in real CI ([#38](https://github.com/nickderobertis/screencomp/pull/38))

## [0.1.12](https://github.com/nickderobertis/screencomp/compare/v0.1.11...v0.1.12) - 2026-06-14

### Fixed

- *(ci)* run the reusable visual-docs workflow's steps under bash so the verify
  step's `set -euo pipefail` works in container jobs whose `/bin/sh` is dash ([#36](https://github.com/nickderobertis/screencomp/pull/36))

## [0.1.11](https://github.com/nickderobertis/screencomp/compare/v0.1.10...v0.1.11) - 2026-06-14

### Added

- *(init)* resolve --platform auto to the host key

### Documentation

- *(examples)* fix manifest auto-push stalling PRs under required checks ([#33](https://github.com/nickderobertis/screencomp/pull/33))

### Other

- Visual-docs DX: reusable workflow, manifest-mode comments, init, doctor mismatch ([#34](https://github.com/nickderobertis/screencomp/pull/34))
- Add optional local pre-push guard and `screencomp scope` ([#32](https://github.com/nickderobertis/screencomp/pull/32))

## [0.1.10](https://github.com/nickderobertis/screencomp/compare/v0.1.9...v0.1.10) - 2026-06-08

### Added

- add verify and doctor commands, interactive-capture guide, and install script ([#30](https://github.com/nickderobertis/screencomp/pull/30))

### Documentation

- link the screencomp-demo consumer; fix just setup on asdf >= 0.16 ([#29](https://github.com/nickderobertis/screencomp/pull/29))

### Added

- *(verify)* add a `verify` subcommand that asserts two captures of the same
  build are byte-identical — the reproducibility gate as a first-class command.
  It exits `3` the moment any shot diverges, labelling each as `differs`,
  `only-in-first`, or `only-in-second`, with a `--format json` contract.
- *(doctor)* add a `doctor` preflight that prints the resolved platform key and
  sanity-checks the `<root>/<project>/<name>.png` layout, flagging an empty
  capture or `.png` files stranded at the root before they surface as a confusing
  empty diff. `--exit-code` turns problems into a non-zero status for CI.

- *(install)* add `scripts/install.sh`, a POSIX install script that detects the
  platform, downloads the matching prebuilt release binary, verifies its SHA-256
  checksum, and installs it to `~/.local/bin` (overridable via `--version`/`--to`
  or `SCREENCOMP_VERSION`/`SCREENCOMP_INSTALL_DIR`). It refuses to install a
  binary it cannot checksum-verify.

### Documentation

- Lead the install docs with the prebuilt-binary install script, then the manual
  release archive and `cargo install --git`; annotate the unpublished crates.io
  line.
- Add a "Capturing an interactive app" guide: explicit Playwright timeouts, the
  determinism-vs-stability flag split (`--single-process` off for interactive
  pages), one browser process per viewport, and a recipe for making
  async/animated widgets byte-reproducible.
- Promote the reproducibility gate to a required, prominently documented step and
  switch the example workflow to `screencomp verify`.

## [0.1.9](https://github.com/nickderobertis/screencomp/compare/v0.1.8...v0.1.9) - 2026-06-07

### Added

- image-free baselines via digest manifest ([#26](https://github.com/nickderobertis/screencomp/pull/26))

### Added

- *(manifest)* add a `manifest` subcommand that writes a screenshot tree's
  digests as a tiny `sha256sum`-style text file, and accept it via
  `--baseline-manifest` on `classify`/`comment`. Committing the manifest instead
  of the baseline PNGs keeps a consuming repository free of unbounded binary
  history while preserving an exact, reviewable record of every shot's digest.

## [0.1.8](https://github.com/nickderobertis/screencomp/compare/v0.1.7...v0.1.8) - 2026-06-06

### Added

- compare screenshots within a platform via --platform ([#23](https://github.com/nickderobertis/screencomp/pull/23))

### Added

- *(classify, gallery, comment)* add `--platform <KEY|auto>` to compare within a
  single `<root>/<platform>/<project>/<name>.png` subtree, so screenshots
  captured on different operating systems or CPU architectures are never
  compared against each other. `auto` resolves to the host's `<os>-<arch>`.
- *(comment)* add `--marker` and `--title` flags overriding `comment.marker` /
  `comment.title`, so a per-platform run can keep one sticky comment each
  without a config file per platform.

## [0.1.7](https://github.com/nickderobertis/screencomp/compare/v0.1.6...v0.1.7) - 2026-06-06

### Added

- *(comment)* embed inline image previews for small diffs ([#10](https://github.com/nickderobertis/screencomp/pull/10))
- *(gallery)* add a before/after diff gallery mode ([#8](https://github.com/nickderobertis/screencomp/pull/8))

### Fixed

- *(gallery)* copy referenced images into the output directory ([#5](https://github.com/nickderobertis/screencomp/pull/5))

### Other

- Initial screencomp CLI for the visual-docs framework

## [0.1.6](https://github.com/nickderobertis/screencomp/compare/v0.1.5...v0.1.6) - 2026-06-06

### Added

- *(comment)* embed inline image previews for small diffs ([#10](https://github.com/nickderobertis/screencomp/pull/10))
- *(gallery)* add a before/after diff gallery mode ([#8](https://github.com/nickderobertis/screencomp/pull/8))

### Fixed

- *(gallery)* copy referenced images into the output directory ([#5](https://github.com/nickderobertis/screencomp/pull/5))

### Other

- Initial screencomp CLI for the visual-docs framework

## [0.1.5](https://github.com/nickderobertis/screencomp/compare/v0.1.4...v0.1.5) - 2026-06-06

### Added

- *(comment)* embed inline image previews for small diffs ([#10](https://github.com/nickderobertis/screencomp/pull/10))
- *(gallery)* add a before/after diff gallery mode ([#8](https://github.com/nickderobertis/screencomp/pull/8))

### Fixed

- *(gallery)* copy referenced images into the output directory ([#5](https://github.com/nickderobertis/screencomp/pull/5))

### Other

- Initial screencomp CLI for the visual-docs framework

## [0.1.4](https://github.com/nickderobertis/screencomp/compare/v0.1.3...v0.1.4) - 2026-06-06

### Added

- *(comment)* embed inline image previews for small diffs ([#10](https://github.com/nickderobertis/screencomp/pull/10))

## [0.1.3](https://github.com/nickderobertis/screencomp/compare/v0.1.2...v0.1.3) - 2026-06-06

### Added

- *(gallery)* add a before/after diff gallery mode ([#8](https://github.com/nickderobertis/screencomp/pull/8))

## [0.1.2](https://github.com/nickderobertis/screencomp/compare/v0.1.1...v0.1.2) - 2026-06-06

### Fixed

- *(gallery)* copy referenced images into the output directory ([#5](https://github.com/nickderobertis/screencomp/pull/5))

## [0.1.1](https://github.com/nickderobertis/screencomp/compare/v0.1.0...v0.1.1) - 2026-06-06

### Other

- Initial screencomp CLI for the visual-docs framework

### Added

- Composite GitHub Action (`action.yml`) that installs the CLI (verified release
  download, or build-from-source) and optionally runs it.
- Multi-arch container image published to GitHub Container Registry on release.
- Example consumer workflow (`examples/`) wiring capture → gallery → Pages →
  sticky PR diff comment.

## [0.1.0]

### Added

- `classify` — compare a current screenshot capture against a baseline and label
  each as added/changed/removed/unchanged by content hash; human and JSON output;
  optional non-zero exit on differences (`--exit-code`).
- `gallery` — render a self-contained static HTML index for a screenshot tree.
- `comment` — render the sticky pull-request comment body for a classification,
  with a stable HTML marker and optional gallery link.
- Optional `screencomp.toml` configuration for the comment command, resolved
  from `--config` or `$SCREENCOMP_CONFIG`.

[Unreleased]: https://github.com/nickderobertis/screencomp/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/nickderobertis/screencomp/releases/tag/v0.1.0
