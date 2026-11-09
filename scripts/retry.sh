#!/usr/bin/env bash
# Bounded retry for tool installers that fetch on their own, where the failure
# cannot be classified from outside (cargo-binstall reports a 504 and a missing
# release with the same exit code):
#   bash scripts/retry.sh <description> <command> [args...]
# A permanent failure costs the short backoff, then fails with the command's own
# output above and its exit code.
set -euo pipefail

if [ "$#" -lt 2 ]; then
  echo "usage: retry.sh <description> <command> [args...]" >&2
  exit 2
fi
description="$1"
shift

max_attempts=4
# Environment knobs exist so tests can shrink the waits.
retry_base_ms="${SCREENCOMP_RETRY_BASE_MS:-5000}"
retry_cap_ms="${SCREENCOMP_RETRY_CAP_MS:-20000}"
for setting in retry_base_ms retry_cap_ms; do
  # Nine digits at most keeps the backoff doubling inside shell arithmetic.
  if ! [[ "${!setting}" =~ ^[1-9][0-9]{0,8}$ ]]; then
    echo "retry.sh: ${setting} must be a positive integer of at most 9 digits, got '${!setting}'" >&2
    exit 1
  fi
done

attempt=1
delay_ms="${retry_base_ms}"
while :; do
  exit_code=0
  "$@" || exit_code=$?
  if [ "${exit_code}" -eq 0 ]; then
    exit 0
  fi
  if [ "${attempt}" -ge "${max_attempts}" ]; then
    echo "${description} failed after ${attempt} attempts (last exit ${exit_code})" >&2
    exit "${exit_code}"
  fi
  if [ "${delay_ms}" -gt "${retry_cap_ms}" ]; then
    delay_ms="${retry_cap_ms}"
  fi
  delay="$(printf '%d.%03d' "$((delay_ms / 1000))" "$((delay_ms % 1000))")"
  echo "${description} failed (exit ${exit_code}) on attempt ${attempt}/${max_attempts}; retrying in ${delay}s" >&2
  sleep "${delay}"
  delay_ms=$((delay_ms * 2))
  attempt=$((attempt + 1))
done
