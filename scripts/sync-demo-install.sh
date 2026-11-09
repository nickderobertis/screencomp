#!/usr/bin/env bash
# sync-demo.yml's "Install the released screencomp" step. A failed install warns
# and exits 0: the baseline reseed skips without the CLI, and the demo refresh
# that follows must still run.
#
# On `release: published` this races release.yml, whose assets arrive a minute
# or two later. The race is waited out by polling the release's asset list, so
# the install that follows can treat a 404 as permanent.
set -euo pipefail

if [ -z "${HAVE_TOKEN:-}" ]; then
  echo "no SCREENCOMP_DEMO_PAT configured; skipping screencomp install"
  exit 0
fi

assets_uploaded() {
  local names asset
  names="$(gh release view "${RELEASE_TAG}" --repo "${GITHUB_REPOSITORY}" \
    --json assets --jq '.assets[] | select(.state == "uploaded") | .name')" || return 1
  while IFS= read -r asset; do
    grep -qxF "${asset}" <<<"${names}" || return 1
  done <<<"${expected_assets}"
}

if [ "${EVENT_NAME:-}" = "release" ]; then
  if ! expected_assets="$(scripts/install.sh --list-assets --version "${RELEASE_TAG}")"; then
    echo "::warning::could not name the assets of release ${RELEASE_TAG}; continuing without screencomp, the baseline reseed is skipped. ACTION: fix the install.sh error above, then rerun this workflow"
    exit 0
  fi
  attempt=1 max=8 delay=15
  until assets_uploaded; do
    if [ "${attempt}" -ge "${max}" ]; then
      echo "::warning::release ${RELEASE_TAG} assets were not uploaded after ${attempt} checks; continuing without screencomp, the baseline reseed is skipped. ACTION: check the release workflow's upload, then rerun this workflow"
      exit 0
    fi
    # llmlint: ignore[tool_output_is_signal] A wait of minutes needs progress in the job log.
    echo "release ${RELEASE_TAG} assets are not uploaded yet (check ${attempt}/${max}); checking again in ${delay}s"
    sleep "${delay}"
    attempt=$((attempt + 1))
    delay=$((delay * 2 > 60 ? 60 : delay * 2))
  done
fi

if SCREENCOMP_INSTALL_DOWNLOADER=scripts/download.sh scripts/install.sh --to "${HOME}/.local/bin"; then
  echo "${HOME}/.local/bin" >>"${GITHUB_PATH}"
else
  echo "::warning::could not install the released screencomp; continuing without it, the baseline reseed is skipped. ACTION: fix the install error above, then rerun this workflow"
fi
