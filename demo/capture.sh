#!/usr/bin/env bash
# Capture the demo's screenshots into $SHOTS_OUT (a captures.json index plus the
# PNGs it references). Single source of truth for the demo's capture, used both by
# the reusable workflow's capture-command AND by screencomp's sync-demo baseline
# reseed, so the two can never diverge. Runs inside the pinned Playwright
# container, where chromium is already present.
set -euo pipefail
npm ci
npx playwright install --with-deps chromium
npx playwright test
