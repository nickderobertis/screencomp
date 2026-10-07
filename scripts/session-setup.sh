#!/usr/bin/env bash
# Claude Code SessionStart hook: a fast, NON-BLOCKING dev-environment check.
#
# It must never run the install itself. Provisioning takes minutes (a full rustup
# toolchain download plus tool installs), and doing that synchronously inside a
# SessionStart hook freezes the session until it finishes — the session waits on
# the hook. So this only runs the lightweight check and, when the environment is
# not ready, prints guidance for the agent to run `just setup` as a visible,
# interruptible first step. Stdout is injected as session context, so a ready
# environment stays silent.
#
# Set SCREENCOMP_AUTO_SETUP=1 to opt into hands-off provisioning: setup is then
# launched detached in the background (still non-blocking) instead of advised.
#
# Every path past the CI and opt-out exits also hands off to
# scripts/setup-llmlint.sh (the llmlint tier: llmlint + oneharness), launched
# detached so it can neither block nor fail the session; its log is
# .dev/setup-llmlint.log. The installer always exits 0, so what it left behind
# is judged instead: .dev/setup-llmlint.status says whether llmlint resolves
# afterwards, and the next session start reports a run that left it missing.
set -eu
# setup.sh installs rust-just so the `just` command surface is available.

# Skip in GitHub Actions CI (workflows provision the toolchain themselves).
# Escape hatch for any other automated context: SCREENCOMP_SKIP_SETUP.
[ -n "${GITHUB_ACTIONS:-}" ] && exit 0
[ -n "${SCREENCOMP_SKIP_SETUP:-}" ] && exit 0

ROOT="${CLAUDE_PROJECT_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)}"
cd "$ROOT"

# Hand off to the llmlint-tier installer, detached: a network install must never
# hold up or fail session start. Registered as the EXIT trap, so each exit below
# reaches it.
handoff_llmlint() {
  [ -x scripts/setup-llmlint.sh ] || return 0
  if ! mkdir -p .dev 2>/dev/null; then
    echo "[screencomp] cannot create .dev/ for the llmlint setup log; run 'just setup-llmlint' by hand" >&2
    return 0
  fi
  if [ "$(cat .dev/setup-llmlint.status 2>/dev/null || true)" = missing ]; then
    printf '%s\n' \
      "[screencomp] The last llmlint setup left llmlint uninstalled (log: .dev/setup-llmlint.log); retrying it in the background." \
      "ACTION: if 'llmlint --version' still fails, run 'just setup-llmlint' to see why and fix what it names."
  fi
  rm -f .dev/setup-llmlint.status
  local launcher="nohup"
  command -v setsid >/dev/null 2>&1 && launcher="setsid"
  # The installer puts llmlint in ~/.local/bin, which may not be on PATH yet.
  # llmlint: ignore[work_goes_through_command_surface] This hook runs before `just` may exist (on a fresh machine it advises installing it), so it calls the installer `just setup-llmlint` wraps directly, as the create-repo session-setup template does.
  "$launcher" bash -c 'export PATH="$HOME/.local/bin:$PATH"; bash scripts/setup-llmlint.sh
      if command -v llmlint >/dev/null 2>&1; then echo ok; else echo missing; fi >.dev/setup-llmlint.status' \
    >.dev/setup-llmlint.log 2>&1 </dev/null &
  return 0
}
trap handoff_llmlint EXIT

# shellcheck source=scripts/setup-lib.sh
. scripts/setup-lib.sh
_load_tool_env

# Ready -> stay silent and cheap.
_check_ready && exit 0

# Opt-in: provision hands-off, but DETACHED so the session is never blocked.
# A flock keeps two concurrent sessions from launching setup twice; the lock is
# held by the background job for its whole run, not by this returning hook.
if [ -n "${SCREENCOMP_AUTO_SETUP:-}" ]; then
  mkdir -p .dev
  launcher="nohup"
  command -v setsid >/dev/null 2>&1 && launcher="setsid"
  "$launcher" bash -c 'exec 9>.dev/setup.lock; flock -n 9 || exit 0; exec bash scripts/setup.sh' \
    >.dev/setup.log 2>&1 </dev/null &
  printf '%s\n' \
    "[screencomp] Dev environment not ready (${REASON}); provisioning in the BACKGROUND" \
    "(log: .dev/setup.log). It does not block this session. Tools appear within a few minutes:" \
    "check with 'just setup-check', then run 'direnv reload' (or open a new shell)."
  exit 0
fi

# Default: advise. Do NOT block the session on a multi-minute install.
printf '%s\n' \
  "[screencomp] Dev environment not set up yet (${REASON})." \
  "ACTION: run 'just setup' (or './scripts/setup.sh' if just is missing) as your FIRST step," \
  "before building or testing. It installs asdf + direnv, the pinned Rust toolchain, and the" \
  "cargo dev tools (several minutes on a fresh machine), then allows the .envrc." \
  "Verify anytime with 'just setup-check'. After it completes, run 'direnv reload' (or open a" \
  "new shell) so asdf and direnv are on PATH."
exit 0
