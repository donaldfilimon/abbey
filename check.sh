#!/bin/sh
# Abbey production gate — fmt + clippy + tests
set -eu
cd "$(dirname "$0")"

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
    elif n > 800:
        print(f"WARN {rel}: {n} lines (soft max 800)")
    elif n > 1000:
        print(f"FAIL {rel}: {n} lines (hard max 1000)")
        bad = True
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
