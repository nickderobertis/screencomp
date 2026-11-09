#!/usr/bin/env bash
# Installs rustup by fetching rustup-init through scripts/download.sh, in place
# of sh.rustup.rs, whose curl has no per-attempt limit and retries only what
# `curl --retry` covers. Arguments go to rustup-init. Run from the repository
# root. RUSTUP_UPDATE_ROOT is rustup's own override of the distribution root.
# llmlint: ignore-file[contracts_have_one_source_or_a_drift_gate] The distribution layout is upstream's published contract, with no source in this repository to share; a change surfaces at once as a 404 naming the URL.
set -euo pipefail

case "$(uname -s)" in
  Linux)
    os="unknown-linux-gnu"
    if ldd --version 2>&1 | grep -qi musl; then
      os="unknown-linux-musl"
    fi
    ;;
  Darwin) os="apple-darwin" ;;
  *)
    echo "install-rustup.sh: unsupported OS $(uname -s). ACTION: install rustup from https://rustup.rs" >&2
    exit 1
    ;;
esac
case "$(uname -m)" in
  x86_64 | amd64) arch="x86_64" ;;
  arm64 | aarch64) arch="aarch64" ;;
  *)
    echo "install-rustup.sh: unsupported architecture $(uname -m). ACTION: install rustup from https://rustup.rs" >&2
    exit 1
    ;;
esac

root="${RUSTUP_UPDATE_ROOT:-https://static.rust-lang.org/rustup}"
work="$(mktemp -d)"
trap 'rm -rf "${work}"' EXIT

bash scripts/download.sh "${root}/dist/${arch}-${os}/rustup-init" "${work}/rustup-init"
chmod +x "${work}/rustup-init"
"${work}/rustup-init" "$@"
