#!/usr/bin/env bash
# Gate a gallery deploy on the GitHub Pages build it triggers actually finishing.
#
# A push to the gallery branch starts a legacy Pages build, and GitHub kills an
# in-flight build the instant a newer one is queued: the superseded build is
# recorded `errored` with a bare `Page build failed.` and duration 0, while the
# identical commit builds fine when nothing races it. Neither the push nor the
# deploy action observes that, so a run could finish green with the site left in
# an `errored` state and the newest gallery never published.
#
# Subcommands:
#   record   Print the id of the Pages build that is current BEFORE the deploy,
#            so `verify` can tell the build the deploy triggers apart from the
#            one already there. Prints nothing when there is no build to read.
#   verify   Wait for a build NEWER than $PREVIOUS_BUILD to appear and settle.
#            `built` passes. Anything else is retried ONCE via a fresh build
#            request — a supersede is transient, so the same commit succeeds
#            unraced — and only then fails the job.
#
# Verification is best effort by design. Observing a build needs `pages: read`
# and requesting one needs `pages: write`; neither is implied by the
# contents:write a deploy already has, and neither may be granted on an external
# gallery repository's token. A token that cannot reach the Pages API warns and
# passes rather than failing every run of a caller that never granted it.
#
# Environment:
#   REPO             owner/name whose Pages build to gate on (required).
#   GH_TOKEN         Token gh authenticates with.
#   GH_BIN           gh executable (default: gh). The seam the test suite drives
#                    the script with a stub; unset in normal CI use.
#   PREVIOUS_BUILD   `record`'s output (verify only). Empty means any build is new.
#   POLL_SECONDS     Delay between polls (default: 10).
#   APPEAR_ATTEMPTS  Extra polls to wait for a NEW build before giving up
#                    (default: 12, so ~120s). Exhausting them means the branch
#                    does not drive a Pages build at all, which warns not fails.
#   SETTLE_ATTEMPTS  Extra polls to wait for a build to leave queued/building
#                    (default: 90, so ~900s).
# The two budgets are counted in polls rather than seconds so the test suite can
# drive every path with POLL_SECONDS=0 and still terminate.
set -euo pipefail

GH_BIN="${GH_BIN:-gh}"
POLL_SECONDS="${POLL_SECONDS:-10}"
APPEAR_ATTEMPTS="${APPEAR_ATTEMPTS:-12}"
SETTLE_ATTEMPTS="${SETTLE_ATTEMPTS:-90}"

die() {
  echo "::error::visual-docs-pages-build: $*" >&2
  exit 1
}

warn() {
  echo "::warning::visual-docs-pages-build: $*" >&2
}

[ -n "${REPO:-}" ] || die "REPO is required"

# Echo "<build-id> <status>" for the newest Pages build, or nothing when the API
# cannot be read — no pages:read on the token, Pages disabled, a Pages source
# that is not a branch, or simply no build yet. Every one of those is a "cannot
# verify", not a failure.
#
# Two single-field reads rather than one composed filter: the build id is only
# the tail of the self URL, and `.url`/`.status` are selectors simple enough to
# be obviously right AND to stub without a jq of the test suite's own.
latest_build() {
  local url status
  url="$("$GH_BIN" api "repos/${REPO}/pages/builds/latest" --jq '.url' 2>/dev/null || true)"
  [ -n "$url" ] || return 0
  status="$("$GH_BIN" api "repos/${REPO}/pages/builds/latest" --jq '.status' 2>/dev/null || true)"
  [ -n "$status" ] || return 0
  printf '%s %s' "${url##*/}" "$status"
}

# Echo the settled status of the first build newer than $PREVIOUS_BUILD, or one
# of the pseudo-statuses `unavailable` (never readable) / `absent` (no new build
# appeared) / `stuck` (never left queued/building).
wait_for_new_build() {
  local attempt build status='' seen=false found=false

  for ((attempt = 0; ; attempt++)); do
    build="$(latest_build)"
    if [ -n "$build" ]; then
      seen=true
      status="${build##* }"
      if [ "${build%% *}" != "${PREVIOUS_BUILD:-}" ]; then
        found=true
        break
      fi
    fi
    [ "$attempt" -lt "$APPEAR_ATTEMPTS" ] || break
    sleep "$POLL_SECONDS"
  done
  if [ "$found" != true ]; then
    if [ "$seen" = true ]; then
      printf 'absent'
    else
      printf 'unavailable'
    fi
    return 0
  fi

  for ((attempt = 0; ; attempt++)); do
    case "$status" in
      queued | building) ;;
      *)
        printf '%s' "$status"
        return 0
        ;;
    esac
    if [ "$attempt" -ge "$SETTLE_ATTEMPTS" ]; then
      printf 'stuck'
      return 0
    fi
    sleep "$POLL_SECONDS"
    build="$(latest_build)"
    [ -n "$build" ] || continue
    status="${build##* }"
  done
}

cmd_record() {
  local build
  build="$(latest_build)"
  [ -n "$build" ] || return 0
  printf '%s\n' "${build%% *}"
}

cmd_verify() {
  local status
  status="$(wait_for_new_build)"
  case "$status" in
    built)
      echo "pages build succeeded for ${REPO}"
      return 0
      ;;
    unavailable)
      warn "cannot read the Pages build status for ${REPO}; grant the deploy token pages:read to gate the run on it"
      return 0
      ;;
    absent)
      warn "no new Pages build appeared for ${REPO} after ${APPEAR_ATTEMPTS} polls; the gallery branch may not be the Pages source"
      return 0
      ;;
    stuck)
      die "the Pages build for ${REPO} was still running after ${SETTLE_ATTEMPTS} polls; the published gallery is stale"
      ;;
  esac

  # A superseded build errors with duration 0 and rebuilds cleanly, so ask for
  # one more before calling the gallery broken.
  echo "pages build for ${REPO} ended '${status}'; requesting a rebuild"
  PREVIOUS_BUILD="$(cmd_record)"
  if ! "$GH_BIN" api --method POST "repos/${REPO}/pages/builds" >/dev/null 2>&1; then
    die "the Pages build for ${REPO} ended '${status}' and a rebuild could not be requested; grant the deploy token pages:write to recover from a superseded build"
  fi
  status="$(wait_for_new_build)"
  [ "$status" = built ] || die "the Pages build for ${REPO} ended '${status}' after a rebuild; the published gallery is stale (see https://github.com/${REPO}/deployments)"
  echo "pages build succeeded for ${REPO} after a rebuild"
}

case "${1:-}" in
  record) cmd_record ;;
  verify) cmd_verify ;;
  *) die "unknown subcommand '${1:-}' (want record|verify)" ;;
esac
