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
#            `built` passes. Otherwise a newer build that settles `built` passes
#            too: the branch content converges, so it publishes this deploy's
#            gallery. Failing that, one fresh build is started — by a build
#            request, or by an empty commit when that request is refused — and
#            only its failure fails the job. A supersede is transient, so the
#            same content succeeds unraced.
#
# Verification is best effort by design. Observing a build needs `pages: read`
# and requesting one needs `pages: write`; neither is implied by the
# contents:write a deploy already has, and neither may be granted on an external
# gallery repository's token. A token that cannot reach the Pages API warns and
# passes rather than failing every run of a caller that never granted it. The
# empty commit uses only that contents:write, so a `pages: read` token can still
# recover.
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
#                    (default: 210, so ~35min). That looks generous next to a
#                    healthy build's ~20-30s, and it is deliberate: a SUPERSEDED
#                    build reports in seconds, but GitHub sits on a build it is
#                    going to fail for content reasons — measured at 13 and 27
#                    minutes on two runs of the e2e fixture. A budget shorter than
#                    that turns an accurate "ended errored" into a vague timeout.
#                    Both fail the job, so this only buys a better diagnosis, and
#                    only in a run that is already failing.
# The two budgets are counted in polls rather than seconds so the test suite can
# drive every path with POLL_SECONDS=0 and still terminate.
set -euo pipefail

GH_BIN="${GH_BIN:-gh}"
POLL_SECONDS="${POLL_SECONDS:-10}"
APPEAR_ATTEMPTS="${APPEAR_ATTEMPTS:-12}"
SETTLE_ATTEMPTS="${SETTLE_ATTEMPTS:-210}"

die() {
  echo "::error::visual-docs-pages-build: $*" >&2
  exit 1
}

warn() {
  echo "::warning::visual-docs-pages-build: $*" >&2
}

command -v "$GH_BIN" >/dev/null 2>&1 ||
  die "GH_BIN '${GH_BIN}' is not an executable command"
[ -n "${REPO:-}" ] || die "REPO is required"
[[ "$REPO" =~ ^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$ ]] ||
  die "REPO must be an owner/name, got '${REPO}'"
# The poll budget is interpolated into arithmetic and `sleep`, so reject anything
# that is not a plain count before it becomes a hang or a shell error.
for knob in POLL_SECONDS APPEAR_ATTEMPTS SETTLE_ATTEMPTS; do
  [[ "${!knob}" =~ ^[0-9]+$ ]] ||
    die "${knob} must be a non-negative whole number, got '${!knob}'"
done

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

# Echo "<status> <build-id>" for the first build newer than $PREVIOUS_BUILD once
# settled, or one of the id-less pseudo-statuses `unavailable` (never readable) /
# `absent` (no new build appeared), or `stuck <build-id>` (never left
# queued/building). The id is the build that status was last read from, which is
# the newest one: a later deploy can replace the build being waited on.
wait_for_new_build() {
  local attempt build id='' status='' seen=false found=false

  for ((attempt = 0; ; attempt++)); do
    build="$(latest_build)"
    if [ -n "$build" ]; then
      seen=true
      status="${build##* }"
      id="${build%% *}"
      if [ "$id" != "${PREVIOUS_BUILD:-}" ]; then
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
        printf '%s %s' "$status" "$id"
        return 0
        ;;
    esac
    if [ "$attempt" -ge "$SETTLE_ATTEMPTS" ]; then
      printf 'stuck %s' "$id"
      return 0
    fi
    sleep "$POLL_SECONDS"
    build="$(latest_build)"
    [ -n "$build" ] || continue
    status="${build##* }"
    id="${build%% *}"
  done
}

die_stuck() {
  die "the Pages build for ${REPO} was still running after ${SETTLE_ATTEMPTS} polls; the published gallery is stale until it finishes — watch it at https://github.com/${REPO}/deployments and re-run this job once it settles"
}

# Start a build using only contents access: an empty commit on the branch Pages
# builds from, made through the Git Data API because the gate has no checkout.
# The ref update is not forced, so it can never overwrite a concurrent deploy.
push_empty_commit() {
  local branch head tree commit
  branch="$("$GH_BIN" api "repos/${REPO}/pages" --jq '.source.branch' 2>/dev/null)" || return 1
  [ -n "$branch" ] || return 1
  head="$("$GH_BIN" api "repos/${REPO}/git/ref/heads/${branch}" --jq '.object.sha' 2>/dev/null)" || return 1
  [ -n "$head" ] || return 1
  tree="$("$GH_BIN" api "repos/${REPO}/git/commits/${head}" --jq '.tree.sha' 2>/dev/null)" || return 1
  [ -n "$tree" ] || return 1
  commit="$("$GH_BIN" api --method POST "repos/${REPO}/git/commits" \
    -f "message=chore: rebuild Pages after a superseded build" \
    -f "tree=${tree}" -f "parents[]=${head}" --jq '.sha' 2>/dev/null)" || return 1
  [ -n "$commit" ] || return 1
  "$GH_BIN" api --method PATCH "repos/${REPO}/git/refs/heads/${branch}" \
    -f "sha=${commit}" >/dev/null 2>&1
}

cmd_record() {
  local build
  build="$(latest_build)"
  [ -n "$build" ] || return 0
  printf '%s\n' "${build%% *}"
}

cmd_verify() {
  local status build newer_status newer_build
  read -r status build <<<"$(wait_for_new_build)"
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
      warn "no new Pages build appeared for ${REPO} after ${APPEAR_ATTEMPTS} polls, so the deploy went unverified; if the gallery should be published, set Settings -> Pages -> Source to 'Deploy from a branch: gh-pages /' on ${REPO}"
      return 0
      ;;
    stuck) die_stuck ;;
  esac

  # A warning, not progress chatter: something raced this deploy, which is worth
  # surfacing even when recovery succeeds. stdout keeps one success line.
  warn "pages build ${build} for ${REPO} ended '${status}', so something raced this deploy; looking for a newer build"

  # GitHub errors a superseded build the moment its successor is queued, so a
  # successor is normally visible at once; none within the appear budget means
  # nothing newer is coming to publish this content.
  PREVIOUS_BUILD="$build"
  read -r newer_status newer_build <<<"$(wait_for_new_build)"
  case "$newer_status" in
    built)
      echo "pages build succeeded for ${REPO} via newer build ${newer_build}, which superseded this deploy's"
      return 0
      ;;
    stuck) die_stuck ;;
    absent | unavailable) ;;
    *)
      warn "newer pages build ${newer_build} for ${REPO} also ended '${newer_status}'"
      status="$newer_status"
      build="$newer_build"
      PREVIOUS_BUILD="$build"
      ;;
  esac

  # A superseded build errors with duration 0 and rebuilds cleanly, so start one
  # more before calling the gallery broken.
  if "$GH_BIN" api --method POST "repos/${REPO}/pages/builds" >/dev/null 2>&1; then
    read -r status build <<<"$(wait_for_new_build)"
    [ "$status" = built ] || die "the Pages build for ${REPO} ended '${status}' after a rebuild; the published gallery is stale (see https://github.com/${REPO}/deployments)"
    echo "pages build succeeded for ${REPO} after a rebuild"
    return 0
  fi

  warn "a rebuild of ${REPO} could not be requested without pages:write; pushing an empty commit to start one"
  push_empty_commit ||
    die "the Pages build ${build} for ${REPO} ended '${status}' and neither a rebuild nor an empty commit could start another; grant the deploy token pages:write to recover from a superseded build"
  read -r status newer_build <<<"$(wait_for_new_build)"
  case "$status" in
    built) ;;
    stuck) die_stuck ;;
    *) die "the Pages build ${newer_build:-after build ${build}} for ${REPO} ended '${status}' after an empty-commit rebuild; the published gallery is stale (see https://github.com/${REPO}/deployments)" ;;
  esac
  echo "pages build succeeded for ${REPO} after an empty-commit rebuild"
}

case "${1:-}" in
  record) cmd_record ;;
  verify) cmd_verify ;;
  *) die "unknown subcommand '${1:-}' (want record|verify)" ;;
esac
