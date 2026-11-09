#!/usr/bin/env bash
# The one download path for this repository's own workflows and recipes:
#   bash scripts/download.sh <url> <destination>
# Fetches to a temporary sibling and renames it into place, so nothing extracts
# or runs a partial file. Transient failures (5xx, 408, 429, timeouts, dropped
# connections) retry with capped exponential backoff; any other failure stops at
# once. The root action.yml carries its own copy of this policy because it must
# work wherever the action is consumed.
#
# SCREENCOMP_DOWNLOAD_BEARER_TOKEN, when set, is sent as an Authorization header
# through curl's stdin config, keeping it out of `ps`.
set -euo pipefail

if [ "$#" -ne 2 ]; then
  echo "usage: download.sh <url> <destination>" >&2
  exit 2
fi
url="$1"
dest="$2"
case "${url}" in
  http://* | https://*) ;;
  *)
    echo "download.sh: expected an http(s) URL, got '${url}'. ACTION: pass the full URL as the first argument" >&2
    exit 2
    ;;
esac

max_attempts=8
# Environment knobs exist so tests can shrink the waits.
attempt_timeout="${SCREENCOMP_DOWNLOAD_ATTEMPT_TIMEOUT:-60}"
retry_base_ms="${SCREENCOMP_DOWNLOAD_RETRY_BASE_MS:-2000}"
retry_cap_ms="${SCREENCOMP_DOWNLOAD_RETRY_CAP_MS:-30000}"

for setting in attempt_timeout retry_base_ms retry_cap_ms; do
  # Nine digits at most keeps the backoff doubling inside shell arithmetic.
  if ! [[ "${!setting}" =~ ^[1-9][0-9]{0,8}$ ]]; then
    echo "download.sh: ${setting} must be a positive integer of at most 9 digits, got '${!setting}'. ACTION: correct or unset the SCREENCOMP_DOWNLOAD_ override" >&2
    exit 1
  fi
done

part="${dest}.part.$$"
trap 'rm -f -- "${part}"' EXIT

# An https URL must not be redirected to anything weaker.
redirect_args=()
case "${url}" in
  https://*) redirect_args=(--proto-redir "=https") ;;
esac

config=""
token="${SCREENCOMP_DOWNLOAD_BEARER_TOKEN:-}"
if [[ "${token}" == *[[:cntrl:]]* ]]; then
  echo "download needs a token without control characters: ${url}. ACTION: correct SCREENCOMP_DOWNLOAD_BEARER_TOKEN" >&2
  exit 1
fi
if [ -n "${token}" ]; then
  # Backslash and double quote are curl config's quoted-value escapes.
  token="${token//\\/\\\\}"
  token="${token//\"/\\\"}"
  config="header = \"Authorization: Bearer ${token}\""
fi

attempt=1
delay_ms="${retry_base_ms}"
while :; do
  exit_code=0
  status="$(curl -fsSL --tlsv1.2 ${redirect_args[@]+"${redirect_args[@]}"} -o "${part}" \
    --config - --connect-timeout 10 --max-time "${attempt_timeout}" \
    --write-out '%{http_code}' "${url}" <<<"${config}")" || exit_code=$?
  if [ "${exit_code}" -eq 0 ]; then
    mv -f -- "${part}" "${dest}"
    exit 0
  fi
  rm -f -- "${part}"

  transient=false
  if [ "${exit_code}" -eq 22 ]; then
    failure="HTTP ${status}"
    case "${status}" in
      408 | 429 | 5??) transient=true ;;
      404)
        echo "download not found (HTTP 404): ${url}. ACTION: check the version and asset name in this URL" >&2
        exit 1
        ;;
    esac
  else
    failure="curl exit ${exit_code}"
    case "${exit_code}" in
      # DNS, connect, HTTP/2, partial body, timeout, TLS handshake, empty reply,
      # send/receive errors, HTTP/2 stream errors.
      5 | 6 | 7 | 16 | 18 | 28 | 35 | 52 | 55 | 56 | 92) transient=true ;;
    esac
  fi
  if [ "${transient}" != true ]; then
    echo "download failed (${failure}): ${url}. ACTION: fix the access or URL problem this error names, then rerun" >&2
    exit 1
  fi
  if [ "${attempt}" -ge "${max_attempts}" ]; then
    echo "download failed after ${attempt} attempts (last error: ${failure}): ${url}. ACTION: check the host's status, then rerun" >&2
    exit 1
  fi
  if [ "${delay_ms}" -gt "${retry_cap_ms}" ]; then
    delay_ms="${retry_cap_ms}"
  fi
  delay="$(printf '%d.%03d' "$((delay_ms / 1000))" "$((delay_ms % 1000))")"
  echo "download failed (${failure}) on attempt ${attempt}/${max_attempts}; retrying in ${delay}s: ${url}" >&2
  sleep "${delay}"
  delay_ms=$((delay_ms * 2))
  attempt=$((attempt + 1))
done
