#!/bin/sh
# Abbey production gate — toolchain, fmt, clippy/tests/rustdoc, claims, installer.
set -eu
cd "$(dirname "$0")"
if [ -z "${ABBEY_PYTHON:-}" ]; then ABBEY_PYTHON=$(command -v python3); fi
export ABBEY_PYTHON

echo "== toolchain =="
./tools/cargo.sh --abbey-qualify-toolchain
ABBEY_TOOLCHAIN_ROOT=$(pwd)
. "$ABBEY_TOOLCHAIN_ROOT/tools/toolchain-env.sh"
cargo check --quiet

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
"$ABBEY_PYTHON" -m unittest discover -s tools/tests -p 'test_*.py'
"$ABBEY_PYTHON" tools/check_claims_sync.py
"$ABBEY_PYTHON" tools/check_instructions.py

# desktop/ is a separate cargo workspace whose TypeScript IPC types are
# generated from src/app_core/ (and desktop/src-tauri/src/{ipc,v3_ipc}.rs) by
# desktop/codegen. Changing those contracts without regenerating breaks the
# desktop client silently, so the root gate runs the generator's `--check`
# mode: it regenerates every output in memory and fails naming each stale file,
# writing nothing. Only the codegen crate is built; the Tauri app, bun, and the
# WebView toolchain are not needed here (the full client gate stays
# desktop/check.sh). `--locked` is deliberate: this gate must never rewrite
# desktop/Cargo.lock. If that lockfile is stale against ../abi or ../wdbx,
# cargo refuses before the generator runs, and the drift check is reported as
# UNMEASURED rather than passed; every other failure is a hard failure.
echo "== desktop IPC codegen drift (--locked; does not build the Tauri app) =="
if codegen_out=$(cargo run --manifest-path desktop/Cargo.toml -p abbey-desktop-codegen --locked --quiet -- --check 2>&1); then
  printf '%s\n' "$codegen_out"
else
  case "$codegen_out" in
    *"cannot update the lock file"*"because --locked was passed"*)
      printf '%s\n' "$codegen_out" >&2
      echo "WARN: desktop/Cargo.lock is stale relative to ../abi or ../wdbx; desktop IPC codegen drift is UNMEASURED, not passed (refresh: cargo update --manifest-path desktop/Cargo.toml --workspace)"
      ;;
    *)
      printf '%s\n' "$codegen_out" >&2
      echo "check.sh: desktop IPC codegen drift check failed (see above)" >&2
      exit 1
      ;;
  esac
fi

echo "== Program 3 read-only boundary =="
"$ABBEY_PYTHON" tools/check_p3_readonly.py

echo "== installer syntax and isolated accelerator layout =="
sh -n install.sh tools/tests/smoke_accel_install.sh
sh tools/tests/smoke_accel_install.sh

echo "== file size guard (main < 200, others warn > 800) =="
"$ABBEY_PYTHON" - <<'PY'
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
