#!/usr/bin/env bash
# Installs cargo-binstall through scripts/download.sh, in place of upstream's
# install-from-binstall-release.sh, whose curl has no per-attempt limit, does not
# retry dropped connections, and pipes straight into tar. Same assets as upstream.
# Run from the repository root. There is deliberately no fallback to
# `cargo-binstall -y --force cargo-binstall`, a second fetch outside download.sh.
# llmlint: ignore-file[contracts_have_one_source_or_a_drift_gate] The asset names are upstream's published contract, with no source in this repository to share; a rename surfaces at once as a 404 naming the URL.
set -euo pipefail

base="${SCREENCOMP_BINSTALL_RELEASE_URL:-https://github.com/cargo-bins/cargo-binstall/releases/latest/download}"

os="$(uname -s)"
machine="$(uname -m)"
case "${os}" in
  Darwin) asset="cargo-binstall-universal-apple-darwin.zip" ;;
  Linux)
    case "${machine}" in
      armv7l) target="armv7-unknown-linux-musleabihf" ;;
      riscv64) target="riscv64gc-unknown-linux-musl" ;;
      *) target="${machine}-unknown-linux-musl" ;;
    esac
    asset="cargo-binstall-${target}.tgz"
    ;;
  MINGW* | MSYS* | CYGWIN*) asset="cargo-binstall-${machine}-pc-windows-msvc.zip" ;;
  *)
    echo "install-cargo-binstall.sh: unsupported OS ${os}. ACTION: install cargo-binstall from https://github.com/cargo-bins/cargo-binstall" >&2
    exit 1
    ;;
esac

work="$(mktemp -d)"
trap 'rm -rf "${work}"' EXIT

bash scripts/download.sh "${base}/${asset}" "${work}/${asset}"
if ! case "${asset}" in
  *.tgz) tar -xzf "${work}/${asset}" -C "${work}" ;;
  *.zip) unzip -q "${work}/${asset}" -d "${work}" ;;
esac; then
  echo "could not extract ${asset}. ACTION: fix the extraction error above, then rerun 'just bootstrap'" >&2
  exit 1
fi
if ! "${work}/cargo-binstall" --self-install; then
  echo "cargo-binstall could not install itself. ACTION: fix the error above, then rerun 'just bootstrap'" >&2
  exit 1
fi
