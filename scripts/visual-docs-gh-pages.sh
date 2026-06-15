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
#   prune-history     Bound $BRANCH's history: keep the KEEP_VERSIONS most recent
#                     commits (gallery versions) intact and collapse everything
#                     older into a single base commit, discarding the accreted blob
#                     history below the window. KEEP_VERSIONS=0 collapses the whole
#                     branch to one fresh commit holding the current tree. A
#                     destructive rewrite of the generated branch only.
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
# prune-history additionally reads:
#   KEEP_VERSIONS  Most recent commits (gallery versions) to keep intact before
#                  collapsing older history into one base commit (default: 20).
#                  0 means keep none — collapse to a single fresh commit.
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

# Clone $BRANCH into $1. The optional $2 is the clone depth (default 1); pass
# "full" for the entire history (prune-history needs it to count and rewrite past
# commits). Returns 1 (without dying) when the branch does not exist, so callers
# can treat that as a clean no-op; any other failure (auth, network) dies loudly
# rather than being mistaken for "nothing to do".
clone_branch() {
  local dest="$1" depth="${2:-1}" url rc=0
  url="$(remote_url)"
  git ls-remote --exit-code --heads "$url" "refs/heads/${BRANCH}" >/dev/null 2>&1 || rc=$?
  if [ "$rc" -eq 2 ]; then
    return 1
  fi
  [ "$rc" -eq 0 ] || die "git ls-remote failed (exit $rc); check GIT_TOKEN/REPO"
  if [ "$depth" = "full" ]; then
    git clone --quiet --branch "$BRANCH" --single-branch "$url" "$dest"
  else
    git clone --quiet --depth "$depth" --branch "$BRANCH" --single-branch "$url" "$dest"
  fi
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
  local keep work total
  keep="${KEEP_VERSIONS:-20}"
  case "$keep" in
    '' | *[!0-9]*) die "KEEP_VERSIONS must be a non-negative integer, got '$keep'" ;;
  esac
  work="$(mktemp -d)"
  # Need the whole history to count versions and rewrite past commits.
  if ! clone_branch "$work" full; then
    echo "no ${BRANCH} branch yet; nothing to prune"
    return 0
  fi
  cd "$work" || die "cannot enter clone $work"
  git config user.name "$BOT_NAME"
  git config user.email "$BOT_EMAIL"

  # keep=0: collapse the whole branch to a single fresh commit holding the current
  # tree, discarding all history (the strongest bound).
  if [ "$keep" -eq 0 ]; then
    git checkout -q --orphan squashed
    git add -A
    git commit -qm "chore(visual-docs): squash gallery history $(date -u +%Y-%m-%d)"
    git push -qf origin "squashed:${BRANCH}"
    echo "squashed ${BRANCH} to a single commit"
    return 0
  fi

  total="$(git rev-list --count HEAD)"
  if [ "$total" -le "$keep" ]; then
    echo "${BRANCH} has ${total} version(s), within the ${keep}-version limit; nothing to prune"
    return 0
  fi

  # Keep the $keep most recent commits intact and collapse everything older into
  # one base commit holding the tree at the cutoff, then replay the kept commits
  # onto it. The kept commits' diffs apply cleanly because the base carries the
  # cutoff's exact tree, so the linear replay never conflicts.
  local cutoff base_tree base_commit
  cutoff="$(git rev-parse "HEAD~${keep}")"
  base_tree="$(git rev-parse "${cutoff}^{tree}")"
  base_commit="$(git commit-tree "$base_tree" \
    -m "chore(visual-docs): squash gallery history before the last ${keep} versions ($(date -u +%Y-%m-%d))")"
  git rebase -q --onto "$base_commit" "$cutoff"
  git push -qf origin "HEAD:${BRANCH}"
  echo "pruned ${BRANCH} to the ${keep} most recent versions (+1 squashed base)"
}

case "${1:-}" in
  cleanup-preview) cmd_cleanup_preview ;;
  prune-history) cmd_prune_history ;;
  *) die "usage: $0 {cleanup-preview|prune-history}" ;;
esac
