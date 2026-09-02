#!/bin/sh
# Abbey production gate — toolchain, fmt, clippy/tests/rustdoc, claims, installer.
set -eu
cd "$(dirname "$0")"

# Toolchain guard. `rust-toolchain.toml` pins an exact nightly, but a real (non-shim)
# cargo earlier on PATH — Homebrew's `rust` formula is the usual culprit —
# silently wins and can be older than the sibling ../abi crates' rust-version.
# Cargo already detects that; what it does not print is the remedy. Probe with
# a cheap unit-graph check rather than comparing version strings, so this does
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
    printf '  Abbey pins   : nightly-2026-09-01 (rust-toolchain.toml)\n' >&2
    cat >&2 <<'MSG'

This is usually PATH shadowing, not a missing toolchain: rustup may already
have the pinned nightly while a Homebrew-installed rustc/cargo appears first
on PATH (rustup's shims live in ~/.cargo/bin and may be absent entirely).

Check and fix:
  rustup run nightly-2026-09-01 rustc --version
  command -v -a cargo                    # which cargo actually wins?
  brew unlink rust                       # let rustup's shims take over
  rustup toolchain install nightly-2026-09-01 --component rustfmt clippy

MSG
    exit 1
  fi
  printf '%s\n' "$probe" >&2
  echo "check.sh: cargo check failed (see above)" >&2
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

# The personal edition is a compile-time cfg: its identity table, path
# resolution, and isolation tests are invisible to the default-feature runs
# above. Gate it explicitly or the separated edition rots unnoticed.
echo "== clippy (--features personal-edition) =="
cargo clippy --features personal-edition --all-targets -- -D warnings

echo "== test (--features personal-edition) =="
cargo test --features personal-edition

# The accelerator bridge (src/accel/bridge.rs) sits behind an off-by-default
# feature, so every run above compiles only its refusal path. Gate it here or
# the kernel/oracle code rots unnoticed. On a host with no Metal device the
# parity tests still pass — they assert the report says `cpu` and claims
# nothing — so this is safe on non-Apple CI. It does need abi-gpu's build
# script to succeed, which on macOS means an Xcode toolchain for `xcrun swiftc`.
echo "== clippy (--features accel) =="
cargo clippy --features accel --all-targets -- -D warnings

echo "== test (--features accel) =="
cargo test --features accel

# Private-item documentation is part of the source contract, and feature-gated
# links can rot independently. Keep each supported product mode explicit so a
# default-only documentation build cannot conceal a broken WDBX, personal, or
# accelerator surface.
echo "== rustdoc (default, warning denied, private items) =="
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --document-private-items

echo "== rustdoc (--features wdbx, warning denied, private items) =="
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --document-private-items --features wdbx

echo "== rustdoc (--features personal-edition, warning denied, private items) =="
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --document-private-items --features personal-edition

echo "== rustdoc (--features accel, warning denied, private items) =="
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --document-private-items --features accel

echo "== claims/docs synchronization =="
python3 -m unittest discover -s tools/tests -p 'test_*.py'
python3 tools/check_claims_sync.py

echo "== Program 3 read-only boundary =="
python3 tools/check_p3_readonly.py

echo "== installer syntax and isolated accelerator layout =="
sh -n install.sh tools/tests/smoke_accel_install.sh
sh tools/tests/smoke_accel_install.sh

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

# Soft coverage report — opt-in only (it re-runs the default suite
# instrumented, roughly doubling test time) and never a hard gate.
if [ "${ABBEY_COVERAGE:-0}" = "1" ]; then
  if command -v cargo-llvm-cov >/dev/null 2>&1; then
    echo "== cargo llvm-cov (soft, default features) =="
    cargo llvm-cov --summary-only \
      || echo "WARN: coverage run failed (llvm-tools missing?) — not a hard gate"
  else
    echo "== skip coverage (cargo-llvm-cov not installed; cargo install cargo-llvm-cov) =="
  fi
fi

echo "check.sh: OK"
