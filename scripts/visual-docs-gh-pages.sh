#!/usr/bin/env bash
# Maintenance for the published visual-docs gallery branch (gh-pages by default).
#
# Single source of truth for two operations that keep the branch from growing
# without bound. The reusable workflow's cleanup-preview / prune-history jobs run
# this script (fetched at their pinned ref), and CI exercises the same code path
# against a disposable branch on the demo consumer — see
# .github/workflows/test-gh-pages-maintenance.yml — so the real logic is covered
# rather than a copy that can drift.
#
# Subcommands:
#   cleanup-preview   Delete pr-$PR/ from $BRANCH. No-op if the branch or the
#                     directory is absent (so a closed PR that never deployed,
#                     or a re-run, is harmless).
#   prune-history     Rewrite $BRANCH to a SINGLE fresh commit holding its current
#                     tree, discarding the accreted blob history. A destructive
#                     rewrite of the generated branch only.
#
# Environment (all subcommands):
#   GIT_TOKEN   Token with contents:write on REPO (required unless REMOTE_URL).
#   REPO        owner/name of the repository to operate on (required unless REMOTE_URL).
#   BRANCH      Branch to maintain (default: gh-pages).
#   GIT_HOST    Git host (default: github.com).
#   REMOTE_URL  Use this remote verbatim instead of building a GitHub https URL
#               from GIT_TOKEN/REPO. The seam that lets the test suite drive the
#               script against a local bare repo; unset in normal CI use.
# cleanup-preview additionally requires:
#   PR          Pull request number whose pr-<n>/ preview to remove.
set -euo pipefail

BRANCH="${BRANCH:-gh-pages}"
GIT_HOST="${GIT_HOST:-github.com}"
readonly BOT_NAME="github-actions[bot]"
readonly BOT_EMAIL="github-actions[bot]@users.noreply.github.com"

die() {
  echo "visual-docs-gh-pages: $*" >&2
  exit 1
}

remote_url() {
  if [ -n "${REMOTE_URL:-}" ]; then
    printf '%s' "$REMOTE_URL"
    return 0
  fi
  [ -n "${GIT_TOKEN:-}" ] || die "GIT_TOKEN is required"
  [ -n "${REPO:-}" ] || die "REPO is required"
  printf 'https://x-access-token:%s@%s/%s.git' "$GIT_TOKEN" "$GIT_HOST" "$REPO"
}

# Clone $BRANCH at depth 1 into $1. Returns 1 (without dying) when the branch
# does not exist, so callers can treat that as a clean no-op; any other failure
# (auth, network) dies loudly rather than being mistaken for "nothing to do".
clone_branch() {
  local dest="$1" url rc=0
  url="$(remote_url)"
  git ls-remote --exit-code --heads "$url" "refs/heads/${BRANCH}" >/dev/null 2>&1 || rc=$?
  if [ "$rc" -eq 2 ]; then
    return 1
  fi
  [ "$rc" -eq 0 ] || die "git ls-remote failed (exit $rc); check GIT_TOKEN/REPO"
  git clone --quiet --depth 1 --branch "$BRANCH" --single-branch "$url" "$dest"
}

cmd_cleanup_preview() {
  [ -n "${PR:-}" ] || die "PR is required for cleanup-preview"
  local work
  work="$(mktemp -d)"
  if ! clone_branch "$work"; then
    echo "no ${BRANCH} branch yet; nothing to clean"
    return 0
  fi
  cd "$work" || die "cannot enter clone $work"
  if [ ! -d "pr-${PR}" ]; then
    echo "no preview at pr-${PR}; nothing to clean"
    return 0
  fi
  git rm -rq "pr-${PR}"
  git config user.name "$BOT_NAME"
  git config user.email "$BOT_EMAIL"
  git commit -qm "chore(visual-docs): drop preview for closed PR #${PR}"
  git push -q origin "$BRANCH"
  echo "removed pr-${PR} from ${BRANCH}"
}

cmd_prune_history() {
  local work
  work="$(mktemp -d)"
  if ! clone_branch "$work"; then
    echo "no ${BRANCH} branch yet; nothing to prune"
    return 0
  fi
  cd "$work" || die "cannot enter clone $work"
  git config user.name "$BOT_NAME"
  git config user.email "$BOT_EMAIL"
  git checkout -q --orphan squashed
  git add -A
  git commit -qm "chore(visual-docs): squash gallery history $(date -u +%Y-%m-%d)"
  git push -qf origin "squashed:${BRANCH}"
  echo "squashed ${BRANCH} to a single commit"
}

case "${1:-}" in
  cleanup-preview) cmd_cleanup_preview ;;
  prune-history) cmd_prune_history ;;
  *) die "usage: $0 {cleanup-preview|prune-history}" ;;
esac
