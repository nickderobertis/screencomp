#!/usr/bin/env bash
# The workspace's locked JavaScript install (package.json + bun.lock: Nx and the
# Playwright suite's dependencies), healed if missing or stale.
#
# A fresh clone, and a CI job that never ran `just bootstrap`, has no
# `node_modules`, so every entry point that needs Nx heals through here rather
# than failing with "cannot find nx". The install always runs under the bun that
# package.json's `packageManager` pins: the one on PATH when it is that version,
# else that exact version through npm (present wherever Node is, which Nx needs).
#
# Quiet on success and idempotent. Anything it says goes to stderr, because a
# caller reading Nx's stdout (`nx show projects --json`) must get only that.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/../.."

if ! command -v node >/dev/null 2>&1; then
    echo "node-modules: Node.js is not installed; Nx runs on it" >&2
    echo "ACTION: install Node.js 22 or newer (https://nodejs.org), then re-run 'just bootstrap'" >&2
    exit 1
fi

version="$(sed -n 's/.*"packageManager": *"bun@\([0-9][0-9.]*\)".*/\1/p' package.json)"
if [ -z "$version" ]; then
    echo "node-modules: package.json pins no bun version in \"packageManager\"" >&2
    echo "ACTION: restore \"packageManager\": \"bun@<version>\" in package.json" >&2
    exit 1
fi

# The install is current while the lockfile and every manifest it was resolved
# from are byte-identical to the ones the last install used.
if ! manifests="$(node -e '
    const root = require("./package.json");
    const members = Array.isArray(root.workspaces) ? root.workspaces : [];
    console.log(["package.json", ...members.map((m) => m + "/package.json")].join("\n"));
' 2>&1)"; then
    echo "node-modules: package.json is not valid JSON: $manifests" >&2
    echo "ACTION: fix package.json, then re-run 'just bootstrap'" >&2
    exit 1
fi
fingerprint() {
    local file
    cat bun.lock
    while IFS= read -r file; do
        printf '\n--- %s\n' "$file"
        cat "$file"
    done <<<"$manifests"
}
stamp=node_modules/.bun-lock-installed
if [ -e node_modules/nx/package.json ] && [ -e "$stamp" ] && cmp -s <(fingerprint) "$stamp"; then
    exit 0
fi

if command -v bun >/dev/null 2>&1 && [ "$(bun --version)" = "$version" ]; then
    bun=(bun)
elif command -v npm >/dev/null 2>&1; then
    bun=(npm exec --yes "--package=bun@$version" -- bun)
else
    echo "node-modules: bun $version (package.json's packageManager) is not on PATH, and there is no npm to fetch it" >&2
    echo "ACTION: install bun $version from https://bun.sh, then re-run 'just bootstrap'" >&2
    exit 1
fi

if ! "${bun[@]}" install --frozen-lockfile --silent >&2; then
    echo "node-modules: 'bun install --frozen-lockfile' (bun $version) failed" >&2
    echo "ACTION: if package.json changed, run 'bun install' and commit bun.lock; otherwise check access to the npm registry" >&2
    exit 1
fi
fingerprint >"$stamp"
