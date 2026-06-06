# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
