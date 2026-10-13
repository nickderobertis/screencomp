#!/usr/bin/env bash
# Install retained hooks and retire only an unchanged lefthook pre-push dispatcher.
set -euo pipefail

lefthook install
pre_commit=$(git rev-parse --git-path hooks/pre-commit)
pre_push=$(git rev-parse --git-path hooks/pre-push)
if [ -f "$pre_commit" ] && [ -f "$pre_push" ]; then
  expected=$(mktemp)
  trap 'rm -f "$expected"' EXIT
  sed 's/call_lefthook run "pre-commit"/call_lefthook run "pre-push"/' "$pre_commit" > "$expected"
  if cmp -s "$expected" "$pre_push"; then
    rm "$pre_push"
  fi
fi
