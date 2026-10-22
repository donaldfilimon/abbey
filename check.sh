#!/bin/sh
# Abbey production gate — toolchain, fmt, clippy/tests/rustdoc, claims, installer.
set -eu
cd "$(dirname "$0")"
if [ -z "${ABBEY_PYTHON:-}" ]; then ABBEY_PYTHON=$(command -v python3); fi
export ABBEY_PYTHON

# One gate at a time per checkout. The integration tests spawn the binaries
# this gate builds, from a fixed path in the shared target directory, so a
# second gate in the same checkout can replace them under a running test. See
# tools/gate_lock.sh.
. ./tools/gate_lock.sh
abbey_gate_lock "$(pwd)"

echo "== toolchain =="
./tools/cargo.sh --abbey-qualify-toolchain
ABBEY_TOOLCHAIN_ROOT=$(pwd)
. "$ABBEY_TOOLCHAIN_ROOT/tools/toolchain-env.sh"
cargo check --quiet --locked

echo "== rustfmt =="
# Package-scoped on purpose. `cargo fmt --all` also formats the *path
# dependencies* — the sibling ../abi and ../wdbx checkouts — so a sibling
# repository's formatting (or its rustfmt edition) could fail Abbey's gate,
# and a plain `cargo fmt` run would silently rewrite the sibling's sources.
# The siblings have their own gates; this one judges Abbey.
cargo fmt --package abbey -- --check

echo "== clippy =="
cargo clippy --locked --all-targets -- -D warnings

echo "== test =="
# Process-level integration tests execute CARGO_BIN_EXE_* artifacts. Build the
# normal binaries explicitly so a prior feature cohort cannot leave a stale
# target/debug executable for the tests to exercise.
cargo build --locked --bins
cargo test --locked

# The wdbx backend is behind an off-by-default feature, so the default-feature
# clippy/test runs above never compile it. Gate it explicitly or it can rot.
#
# The feature cohorts are scoped to the packages that *own* the feature. The
# desktop client has no `wdbx` and no `accel` feature, and rebuilding the
# Tauri dependency tree for a cohort that cannot use it costs minutes and
# gigabytes of disk on every run. The default cohort above still builds and
# tests the whole workspace, including the client.
echo "== clippy (--features wdbx) =="
cargo clippy --locked -p abbey --features wdbx --all-targets -- -D warnings

echo "== test (--features wdbx) =="
cargo build --locked -p abbey --features wdbx --bins
cargo test --locked -p abbey --features wdbx

# The personal edition is a compile-time cfg: its identity table, path
# resolution, and isolation tests are invisible to the default-feature runs
# above. Gate it explicitly or the separated edition rots unnoticed. The
# desktop client has a matching feature (it selects the edition's bundle
# identity), so it is gated with it.
echo "== clippy (--features personal-edition) =="
cargo clippy --locked -p abbey -p abbey-desktop --features personal-edition --all-targets -- -D warnings

echo "== test (--features personal-edition) =="
cargo build --locked -p abbey -p abbey-desktop --features personal-edition --bins
cargo test --locked -p abbey -p abbey-desktop --features personal-edition

# The accelerator bridge (src/accel/bridge.rs) sits behind an off-by-default
# feature, so every run above compiles only its refusal path. Gate it here or
# the kernel/oracle code rots unnoticed. On a host with no Metal device the
# parity tests still pass — they assert the report says `cpu` and claims
# nothing — so this is safe on non-Apple CI. It does need abi-gpu's build
# script to succeed, which on macOS means an Xcode toolchain for `xcrun swiftc`.
echo "== clippy (--features accel) =="
cargo clippy --locked -p abbey --features accel --all-targets -- -D warnings

echo "== test (--features accel) =="
cargo build --locked -p abbey --features accel --bins
cargo test --locked -p abbey --features accel

# Private-item documentation is part of the source contract, and feature-gated
# links can rot independently. Keep each supported product mode explicit so a
# default-only documentation build cannot conceal a broken WDBX, personal, or
# accelerator surface. The default build documents every workspace member.
echo "== rustdoc (default, warning denied, private items) =="
RUSTDOCFLAGS="-D warnings" cargo doc --locked --no-deps --document-private-items

echo "== rustdoc (--features wdbx, warning denied, private items) =="
RUSTDOCFLAGS="-D warnings" cargo doc --locked -p abbey --no-deps --document-private-items --features wdbx

echo "== rustdoc (--features personal-edition, warning denied, private items) =="
RUSTDOCFLAGS="-D warnings" cargo doc --locked -p abbey -p abbey-desktop --no-deps --document-private-items --features personal-edition

echo "== rustdoc (--features accel, warning denied, private items) =="
RUSTDOCFLAGS="-D warnings" cargo doc --locked -p abbey --no-deps --document-private-items --features accel

echo "== claims/docs synchronization =="
"$ABBEY_PYTHON" -m unittest discover -s tools/tests -p 'test_*.py'
"$ABBEY_PYTHON" tools/check_claims_sync.py
"$ABBEY_PYTHON" tools/check_instructions.py

# The desktop client's TypeScript IPC types are generated from src/app_core/
# (and desktop/src-tauri/src/{ipc,v3_ipc}.rs) by desktop/codegen. Changing
# those contracts without regenerating breaks the desktop client silently, so
# the root gate runs the generator's `--check` mode: it regenerates every
# output in memory and fails naming each stale file, writing nothing.
#
# The codegen crate is a member of this workspace, so the generator is built
# from the same lockfile as everything else. `--locked` is deliberate: this
# gate must never rewrite Cargo.lock. If that lockfile is stale against
# ../abi or ../wdbx, cargo refuses before the generator runs; every failure
# here is a hard failure, because the rest of the gate already depends on the
# same lockfile being current.
echo "== desktop IPC codegen drift (--locked) =="
if codegen_out=$(cargo run --locked --quiet -p abbey-desktop-codegen -- --check 2>&1); then
  printf '%s\n' "$codegen_out"
else
  printf '%s\n' "$codegen_out" >&2
  echo "check.sh: desktop IPC codegen drift check failed (see above)" >&2
  exit 1
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

# Soft cross-compile smoke (Linux/Windows guard): delegated to prepared script.
# Does not download toolchains — keeps the gate offline-friendly.
# These are soft checks (never hard gate). Part of Phase 2 / working-ci
# readiness for portable surfaces. Script is executable and self-contained.
sh tools/ci/cross-compile-guard.sh || echo "WARN: cross guard script non-zero (soft, continuing)"

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
