#!/bin/sh
# Prove the accelerator package from an isolated installed layout, including
# rollback when staged Mach-O verification fails.
set -eu

ROOT=$(CDPATH= cd -- "$(dirname "$0")/../.." && pwd)
cd "$ROOT"

if [ "$(uname -s 2>/dev/null || printf unknown)" != "Darwin" ]; then
  echo "accelerator install smoke: SKIP (verified Mach-O layout requires macOS)"
  exit 0
fi

SCRATCH=$(mktemp -d "${TMPDIR:-/tmp}/abbey-accel-install.XXXXXX")
cleanup() {
  rm -rf -- "$SCRATCH"
}
trap cleanup EXIT HUP INT TERM

SUCCESS_DIR="$SCRATCH/success/bin"
SUCCESS_HOME="$SCRATCH/success/home"
mkdir -p "$SUCCESS_DIR" "$SUCCESS_HOME"
ABBEY_CARGO_FEATURES=accel \
ABBEY_INSTALL_DIR="$SUCCESS_DIR" \
ABBEY_COMPLETION_HOME="$SUCCESS_HOME" \
  ./install.sh

INSTALLED_BIN="$SUCCESS_DIR/abbey"
INSTALLED_DAEMON="$SUCCESS_DIR/abbeyd"
INSTALLED_DYLIB="$SUCCESS_DIR/libabi_metal_dot.dylib"
test -x "$INSTALLED_BIN"
test -x "$INSTALLED_DAEMON"
test -f "$INSTALLED_DYLIB"
test ! -L "$INSTALLED_DYLIB"
otool -D "$INSTALLED_DYLIB" | grep -Fqx '@loader_path/libabi_metal_dot.dylib'
otool -L "$INSTALLED_BIN" | grep -Fq '@loader_path/libabi_metal_dot.dylib ('
otool -L "$INSTALLED_DAEMON" | grep -Fq '@loader_path/libabi_metal_dot.dylib ('
"$INSTALLED_BIN" --version >/dev/null
if daemon_output=$(
  unset ABBEYD_BEARER_TOKEN ABBEYD_BEARER_TOKEN_FILE \
    ABBEY_PERSONAL_DAEMON_BEARER_TOKEN \
    ABBEY_PERSONAL_DAEMON_BEARER_TOKEN_FILE
  "$INSTALLED_DAEMON" 2>&1
); then
  echo "accelerator install smoke: daemon unexpectedly started without a bearer" >&2
  exit 1
fi
printf '%s\n' "$daemon_output" |
  grep -Eq '^abbeyd: set exactly one of [A-Z0-9_]+_BEARER_TOKEN or [A-Z0-9_]+_BEARER_TOKEN_FILE$'
"$INSTALLED_BIN" accel verify --json | python3 -c '
import json, sys
report = json.load(sys.stdin)
assert report["kernels_linked"] is True
assert report["backend_used"] in {"cpu", "gpu-metal", "mixed"}
if __import__("os").environ.get("ABBEY_REQUIRE_NATIVE_ACCEL") == "1":
    assert report["device_initialized"] is True
    assert report["backend_used"] == "gpu-metal"
    assert report["verified"] is True
    assert all(item["executed_natively"] for item in report["kernels"])
    assert all(item["oracle_parity"] for item in report["kernels"])
'

# Existing install fixtures and their byte-for-byte snapshots.
prepare_old_install() {
  fixture_dir="$1"
  snapshot_dir="$2"
  mkdir -p "$fixture_dir" "$snapshot_dir"
  printf 'old abbey binary\n' > "$fixture_dir/abbey"
  printf 'old abbeyd binary\n' > "$fixture_dir/abbeyd"
  printf 'old ABI Metal dylib\n' > "$fixture_dir/libabi_metal_dot.dylib"
  cp "$fixture_dir/abbey" "$snapshot_dir/abbey"
  cp "$fixture_dir/abbeyd" "$snapshot_dir/abbeyd"
  cp "$fixture_dir/libabi_metal_dot.dylib" "$snapshot_dir/libabi_metal_dot.dylib"
  chmod 755 "$fixture_dir/abbey" "$fixture_dir/abbeyd"
}

assert_old_install_unchanged() {
  fixture_dir="$1"
  snapshot_dir="$2"
  cmp "$snapshot_dir/abbey" "$fixture_dir/abbey"
  cmp "$snapshot_dir/abbeyd" "$fixture_dir/abbeyd"
  cmp "$snapshot_dir/libabi_metal_dot.dylib" "$fixture_dir/libabi_metal_dot.dylib"
  if find "$fixture_dir" -maxdepth 1 \
    \( -name '.abbey-install.*' -o -name '.abbey-backup.*' \) | grep -q .; then
    echo "accelerator install smoke: transactional residue survived failure" >&2
    exit 1
  fi
}

# The first three otool calls verify the release source dylib, CLI, and daemon.
# Fail the fourth call, which is the staged dylib check. Existing destination
# bytes must remain exact and every staging/backup directory must be removed.
FAIL_DIR="$SCRATCH/failure/bin"
FAIL_HOME="$SCRATCH/failure/home"
SNAPSHOT_DIR="$SCRATCH/failure/snapshot"
FAKE_BIN="$SCRATCH/fake-bin"
mkdir -p "$FAIL_HOME" "$FAKE_BIN"
prepare_old_install "$FAIL_DIR" "$SNAPSHOT_DIR"

ABBEY_REAL_OTOOL=$(command -v otool)
export ABBEY_REAL_OTOOL
ABBEY_OTOOL_STATE="$SCRATCH/otool-count"
export ABBEY_OTOOL_STATE
ABBEY_REAL_MV=$(command -v mv)
export ABBEY_REAL_MV
ABBEY_MV_STATE="$SCRATCH/mv-count"
export ABBEY_MV_STATE
{
  printf '%s\n' '#!/bin/sh' 'set -eu'
  printf '%s\n' 'count=0'
  printf '%s\n' 'if [ -f "$ABBEY_OTOOL_STATE" ]; then count=$(sed -n "1p" "$ABBEY_OTOOL_STATE"); fi'
  printf '%s\n' 'count=$((count + 1))'
  printf '%s\n' 'printf "%s\n" "$count" > "$ABBEY_OTOOL_STATE"'
  printf '%s\n' 'if [ "${ABBEY_OTOOL_FAIL_AT:-0}" -eq "$count" ]; then exit 86; fi'
  printf '%s\n' 'exec "$ABBEY_REAL_OTOOL" "$@"'
} > "$FAKE_BIN/otool"
{
  printf '%s\n' '#!/bin/sh' 'set -eu'
  printf '%s\n' 'count=0'
  printf '%s\n' 'if [ -f "$ABBEY_MV_STATE" ]; then count=$(sed -n "1p" "$ABBEY_MV_STATE"); fi'
  printf '%s\n' 'count=$((count + 1))'
  printf '%s\n' 'printf "%s\n" "$count" > "$ABBEY_MV_STATE"'
  printf '%s\n' 'if [ "${ABBEY_MV_FAIL_AT:-0}" -eq "$count" ]; then exit 87; fi'
  printf '%s\n' 'exec "$ABBEY_REAL_MV" "$@"'
} > "$FAKE_BIN/mv"
chmod 755 "$FAKE_BIN/otool" "$FAKE_BIN/mv"

if PATH="$FAKE_BIN:$PATH" \
  ABBEY_OTOOL_FAIL_AT=4 \
  ABBEY_MV_FAIL_AT=0 \
  ABBEY_CARGO_FEATURES=accel \
  ABBEY_INSTALL_DIR="$FAIL_DIR" \
  ABBEY_COMPLETION_HOME="$FAIL_HOME" \
  ./install.sh; then
  echo "accelerator install smoke: expected staged verification failure" >&2
  exit 1
fi
assert_old_install_unchanged "$FAIL_DIR" "$SNAPSHOT_DIR"

# With all verification enabled, fail the fifth move: three existing files
# have been backed up and the new dylib has been published, but publishing the
# daemon fails. The trap must remove the new dylib and restore all three old
# files byte-for-byte.
RELOCATE_DIR="$SCRATCH/relocate-failure/bin"
RELOCATE_HOME="$SCRATCH/relocate-failure/home"
RELOCATE_SNAPSHOT="$SCRATCH/relocate-failure/snapshot"
mkdir -p "$RELOCATE_HOME"
prepare_old_install "$RELOCATE_DIR" "$RELOCATE_SNAPSHOT"
rm -f "$ABBEY_OTOOL_STATE" "$ABBEY_MV_STATE"
if PATH="$FAKE_BIN:$PATH" \
  ABBEY_OTOOL_FAIL_AT=0 \
  ABBEY_MV_FAIL_AT=5 \
  ABBEY_CARGO_FEATURES=accel \
  ABBEY_INSTALL_DIR="$RELOCATE_DIR" \
  ABBEY_COMPLETION_HOME="$RELOCATE_HOME" \
  ./install.sh; then
  echo "accelerator install smoke: expected relocation failure" >&2
  exit 1
fi
assert_old_install_unchanged "$RELOCATE_DIR" "$RELOCATE_SNAPSHOT"

echo "accelerator install smoke: OK (isolated layout + verification/relocation rollback)"
