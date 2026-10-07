#!/usr/bin/env bash
# Fail the `llmlint` CI job fast, and say what to do, when the codex harness's
# credential is missing or rejected; otherwise authenticate codex with it. The
# policy (why there is no skip path, which secret) is llmlint.yml's header.
set -euo pipefail

if [ -z "${OPENAI_API_KEY:-}" ]; then
    echo "::error::llmlint needs the OPENAI_API_KEY repository secret (the codex harness's credential; oneharness.toml names codex first). Add it under Settings → Secrets and variables → Actions, then re-run this job." >&2
    exit 1
fi

if ! command -v codex >/dev/null 2>&1; then
    echo "::error::the codex harness is not installed; the workflow's install step must run before this one" >&2
    exit 1
fi
if ! printenv OPENAI_API_KEY | codex login --with-api-key >/dev/null; then
    echo "::error::codex rejected the OPENAI_API_KEY repository secret; replace it with a valid OpenAI API key under Settings → Secrets and variables → Actions, then re-run this job" >&2
    exit 1
fi
echo "codex authenticated from OPENAI_API_KEY"
