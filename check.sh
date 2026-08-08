#!/bin/sh
# Abbey production gate — fmt + clippy + tests
set -eu
cd "$(dirname "$0")"

# Toolchain guard. `rust-toolchain.toml` pins nightly, but a real (non-shim)
# cargo earlier on PATH — Homebrew's `rust` formula is the usual culprit —
# silently wins and can be older than the sibling ../abi crates' rust-version.
# Cargo already detects that; what it does not print is the remedy. Probe with
# a cheap metadata resolve rather than comparing version strings, so this does
# not hardcode a version that rots the next time ../abi bumps.
# `cargo check` is the probe because the rust-version gate fires during unit-graph
# construction, before any compilation — so in the failing case it returns
# immediately, and in the passing case it warms dev-profile artifacts that the
# `cargo test` steps below reuse.
echo "== toolchain =="
if ! probe=$(cargo check --quiet 2>&1 >/dev/null); then
  if printf '%s' "$probe" | grep -q 'is not supported by the following packages'; then
    printf '%s\n' "$probe" >&2
    printf '\ncheck.sh: the active toolchain is too old for this workspace.\n\n' >&2
    printf '  active cargo : %s (%s)\n' "$(command -v cargo)" "$(cargo --version 2>&1)" >&2
    printf '  Abbey pins   : nightly (rust-toolchain.toml)\n' >&2
    cat >&2 <<'MSG'

This is usually PATH shadowing, not a missing toolchain: rustup may already
have a new-enough nightly while a Homebrew-installed rustc/cargo appears first
on PATH (rustup's shims live in ~/.cargo/bin and may be absent entirely).

Check and fix:
  rustup run nightly rustc --version     # is rustup's nightly new enough?
  command -v -a cargo                    # which cargo actually wins?
  brew unlink rust                       # let rustup's shims take over
  rustup default nightly                 # (re)install shims if missing

MSG
    exit 1
  fi
  printf '%s\n' "$probe" >&2
  echo "check.sh: cargo metadata failed (see above)" >&2
  exit 1
fi
echo "ok ($(cargo --version))"

echo "== rustfmt =="
cargo fmt --all -- --check

echo "== clippy =="
cargo clippy --all-targets -- -D warnings

echo "== test =="
cargo test

# The wdbx backend is behind an off-by-default feature, so the default-feature
# clippy/test runs above never compile it. Gate it explicitly or it can rot.
echo "== clippy (--features wdbx) =="
cargo clippy --features wdbx --all-targets -- -D warnings

echo "== test (--features wdbx) =="
cargo test --features wdbx

echo "== claims/docs synchronization =="
python3 tools/check_claims_sync.py

echo "== file size guard (main < 200, others warn > 800) =="
python3 - <<'PY'
from pathlib import Path
bad = False
for p in Path("src").rglob("*.rs"):
    n = sum(1 for _ in p.open())
    rel = str(p)
    if p.name == "main.rs" and n > 200:
        print(f"FAIL {rel}: {n} lines (max 200)")
        bad = True
    elif n > 1000:
        print(f"FAIL {rel}: {n} lines (hard max 1000)")
        bad = True
    elif n > 800:
        print(f"WARN {rel}: {n} lines (soft max 800)")
if bad:
    raise SystemExit(1)
print("ok")
PY

# Soft cross-compile smoke: only when the target is already installed.
# Does not download toolchains — keeps the gate offline-friendly.
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
esac

echo "check.sh: OK"
