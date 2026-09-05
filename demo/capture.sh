#!/usr/bin/env bash
# Capture the demo's screenshots into $SHOTS_OUT (a captures.json index plus the
# PNGs it references). Single source of truth for the demo's capture, used both by
# the reusable workflow's capture-command AND by screencomp's sync-demo baseline
# reseed, so the two can never diverge. Runs inside the pinned Playwright
# container, where chromium is already present.
#
# `install` without `--with-deps`: both callers now run the container as the
# invoking host user so it cannot leave root-owned files in the bind-mounted
# tree, and `--with-deps` shells out to apt as root, which that user is not. The
# image already ships chromium and its system dependencies, so the flag only ever
# re-confirmed what the pin guarantees.
set -euo pipefail
npm ci
# llmlint: ignore[changed_behavior_has_e2e] This install path runs only inside the pinned container, which no offline test starts; the demo journey AGENTS.md requires before release executes this exact script there as the invoking uid, and it installs and captures with byte-identical PNGs.
npx playwright install chromium
npx playwright test
