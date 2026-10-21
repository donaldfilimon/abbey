#!/bin/sh
# Soft Linux/Windows cross-compile guard.
# Invoked from check.sh (and can be run standalone).
# Skips when targets not installed (no download, offline friendly).
# Never a hard gate; warns only.
# Prepares the portable surface evidence path for the blocked
# ci-self-hosted-linux-proof / working-ci item (Linux + Windows runtime).
# Part of Phase 2 guard preparation for portable surfaces.
set -eu

cross_check() {
  target="$1"
  if rustup target list --installed 2>/dev/null | grep -qx "$target"; then
    echo "== cargo check --target ${target} (soft) =="
    if ! cargo check --target "$target" --quiet; then
      echo "WARN: cross-check ${target} failed (linker/sysroot?) — not a hard gate"
      return 0
    fi
    if ! cargo check --features wdbx --target "$target" --quiet; then
      echo "WARN: cross-check ${target} +wdbx failed — not a hard gate"
    fi
  else
    echo "== skip cross-check ${target} (not installed) =="
  fi
}

# From macOS/linux hosts, optionally verify the Windows + other-unix portable set.
case "$(uname -s 2>/dev/null || echo unknown)" in
  Darwin|Linux)
    cross_check x86_64-pc-windows-gnu
    cross_check x86_64-unknown-linux-gnu
    ;;
  *)
    echo "== skip cross-compile guard (non-Darwin/Linux host) =="
    ;;
esac
