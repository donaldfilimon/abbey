#!/usr/bin/env bash
# Matched repository-selected Rust tools, preserving ordinary developer overrides.
set -euo pipefail
ABBEY_TOOLCHAIN_ROOT=$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
if [ "${1:-}" = --abbey-qualify-toolchain ]; then
  "${ABBEY_PYTHON:-$(command -v python3)}" "$ABBEY_TOOLCHAIN_ROOT/tools/toolchain_policy.py"
fi
. "$ABBEY_TOOLCHAIN_ROOT/tools/toolchain-env.sh"
if [ "${1:-}" = --abbey-qualify-toolchain ]; then
  printf 'compiler: %s\n' "$ABBEY_TOOLCHAIN_BIN/rustc"
  compiler_version=$("$ABBEY_TOOLCHAIN_BIN/rustc" -vV)
  printf '%s\n' "$compiler_version"
  printf '%s\n' "$compiler_version" | /usr/bin/grep -qx 'release: 1.99.1' || {
    echo 'stable qualification requires rustc release 1.99.1' >&2
    exit 1
  }
  "$ABBEY_TOOLCHAIN_CARGO" --version
  exit 0
fi
exec "$ABBEY_TOOLCHAIN_CARGO" "$@"
