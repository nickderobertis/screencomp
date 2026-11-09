#!/usr/bin/env bash
# Runs cargo-binstall with bounded retries for transient failures only:
#   bash scripts/binstall.sh <cargo-binstall args...>
# cargo-binstall exits 94 for both a 504 and a missing release, so the outcome
# is read from its log. The patterns are the default-level messages it emits for
# a 408/429/5xx, a dropped connection and a resolution timeout; the suite holds
# them against the real binary. Anything unrecognized is permanent: one attempt.
set -euo pipefail

max_attempts=4
# Environment knobs exist so tests can shrink the waits.
attempt_timeout="${SCREENCOMP_BINSTALL_ATTEMPT_TIMEOUT:-300}"
retry_base_ms="${SCREENCOMP_BINSTALL_RETRY_BASE_MS:-5000}"
retry_cap_ms="${SCREENCOMP_BINSTALL_RETRY_CAP_MS:-20000}"
for setting in attempt_timeout retry_base_ms retry_cap_ms; do
  # Nine digits at most keeps the backoff doubling inside shell arithmetic.
  if ! [[ "${!setting}" =~ ^[1-9][0-9]{0,8}$ ]]; then
    echo "binstall.sh: ${setting} must be a positive integer of at most 9 digits, got '${!setting}'" >&2
    exit 1
  fi
done

transient_log='Received status code (408|429|5[0-9][0-9])|HTTP status server error|Timeout reached while checking fetcher|reqwest::Error \{ kind: (Request|Connect|Body)|error sending request|hyper::Error|operation timed out|connection (reset|closed|refused|aborted)|dns error'

work="$(mktemp -d)"
trap 'rm -rf "${work}"' EXIT
log="${work}/attempt.log"
expired="${work}/expired"

# Job control gives each background job its own process group, so the watchdog
# can stop the whole attempt and its own pending sleep.
set -m

kill_group() {
  kill "-$2" -- "-$1" 2>/dev/null || kill "-$2" "$1" 2>/dev/null || true
}

attempt=1
delay_ms="${retry_base_ms}"
while :; do
  rm -f "${expired}"
  cargo-binstall "$@" >"${log}" 2>&1 </dev/null &
  installer=$!
  (
    sleep "${attempt_timeout}"
    : >"${expired}"
    kill_group "${installer}" TERM
    sleep 5
    kill_group "${installer}" KILL
  ) >/dev/null 2>&1 &
  watchdog=$!

  exit_code=0
  wait "${installer}" || exit_code=$?
  kill_group "${watchdog}" KILL
  wait "${watchdog}" 2>/dev/null || true
  cat "${log}"

  if [ "${exit_code}" -eq 0 ]; then
    exit 0
  fi

  if [ -e "${expired}" ]; then
    exit_code=124
    failure="timed out after ${attempt_timeout}s"
  elif grep -Eq "${transient_log}" "${log}"; then
    failure="transient error, exit ${exit_code}"
  else
    echo "cargo-binstall failed with a permanent error (exit ${exit_code}), see its output above" >&2
    exit "${exit_code}"
  fi

  if [ "${attempt}" -ge "${max_attempts}" ]; then
    echo "cargo-binstall failed after ${attempt} attempts (last error: ${failure})" >&2
    exit "${exit_code}"
  fi
  if [ "${delay_ms}" -gt "${retry_cap_ms}" ]; then
    delay_ms="${retry_cap_ms}"
  fi
  delay="$(printf '%d.%03d' "$((delay_ms / 1000))" "$((delay_ms % 1000))")"
  echo "cargo-binstall failed (${failure}) on attempt ${attempt}/${max_attempts}; retrying in ${delay}s" >&2
  sleep "${delay}"
  delay_ms=$((delay_ms * 2))
  attempt=$((attempt + 1))
done
