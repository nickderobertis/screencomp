#!/usr/bin/env bash
# Fail the `llmlint` CI job fast, and say what to do, when the harness credential
# is missing; otherwise authenticate the harness with it.
#
# The judged tier never passes without judging: a green run with no credential
# would report every changed file as clean. The harness is codex, the primary in
# oneharness.toml's fallback chain, and its credential is the repository secret
# OPENAI_API_KEY (AGENTS.md, "Release & git"). Fork pull requests get no secrets,
# so they fail here too; the repository's require-approval-for-fork-workflows
# setting is what gates them, not a no-op branch.
set -euo pipefail

if [ -z "${OPENAI_API_KEY:-}" ]; then
    echo "::error::llmlint needs the OPENAI_API_KEY repository secret (the codex harness's credential; oneharness.toml names codex first). Add it under Settings → Secrets and variables → Actions, then re-run this job." >&2
    exit 1
fi

if ! command -v codex >/dev/null 2>&1; then
    echo "::error::the codex harness is not installed; the workflow's install step must run before this one" >&2
    exit 1
fi
printenv OPENAI_API_KEY | codex login --with-api-key >/dev/null
echo "codex authenticated from OPENAI_API_KEY"
