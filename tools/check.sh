#!/bin/sh
# abbey-zig gate: the single source of truth for "green".
# Stages are added as the phase lands; every stage fails the gate.
set -eu
cd "$(dirname "$0")/.."
LOG_DIR="${ABBEY_ZIG_GATE_LOG_DIR:-/private/tmp/abbey-zig-gate}"
mkdir -p "$LOG_DIR"

stage() { printf '== %s ==\n' "$1"; }

stage "zig version"
zig version

stage "fmt --check"
zig fmt --check src build.zig

stage "build (safe edition)"
zig build -Dpersonal=false > "$LOG_DIR/build-safe.log" 2>&1 || { cat "$LOG_DIR/build-safe.log"; exit 1; }
test -x zig-out/bin/abbey-zig
test -x zig-out/bin/abbeyd-zig

stage "build (personal edition)"
zig build -Dpersonal=true > "$LOG_DIR/build-personal.log" 2>&1 || { cat "$LOG_DIR/build-personal.log"; exit 1; }
test -x zig-out/bin/abbey-zig-personal
test -x zig-out/bin/abbeyd-zig-personal

run_tests() {
  # $1 = true|false for -Dpersonal, $2 = binary prefix
  zig build test-bin -Dpersonal="$1" > "$LOG_DIR/test-bin-$2.log" 2>&1 || { cat "$LOG_DIR/test-bin-$2.log"; exit 1; }
  for kind in lib main; do
    log="$LOG_DIR/tests-$2-$kind.log"
    if ! "./zig-out/bin/$2-$kind-tests" > "$log" 2>&1; then cat "$log"; echo "FAIL: $2 $kind tests"; exit 1; fi
    if grep -qE "leaked .* allocated at|SafeAllocator leaked|tests leaked memory" "$log"; then cat "$log"; echo "FAIL: leak report in $2 $kind tests"; exit 1; fi
    line=$(grep -E "^All [0-9]+ tests passed" "$log" || true)
    [ -n "$line" ] || { cat "$log"; echo "FAIL: no pass line in $2 $kind tests"; exit 1; }
    echo "$2 $kind: $line"
  done
}

stage "rebuild safe edition for the binary stages"
zig build -Dpersonal=false > "$LOG_DIR/build-safe2.log" 2>&1 || { cat "$LOG_DIR/build-safe2.log"; exit 1; }

stage "tests, safe edition (std.testing.allocator leak detection)"
run_tests false abbey-zig

stage "tests, personal edition (std.testing.allocator leak detection)"
run_tests true abbey-zig-personal

stage "help goldens"
for g in tests/golden/help/*.txt; do
  c=$(basename "$g" .txt)
  ./zig-out/bin/abbey-zig "$c" --help > "$LOG_DIR/help-$c.txt" 2>&1 || { echo "FAIL: $c --help exit"; exit 1; }
  diff "$g" "$LOG_DIR/help-$c.txt" > /dev/null || { diff "$g" "$LOG_DIR/help-$c.txt"; echo "FAIL: help golden $c"; exit 1; }
done
echo "ok: $(ls tests/golden/help/*.txt | wc -l | tr -d ' ') goldens"

stage "contracts qualification"
python3 tools/abbey_contracts.py verify contracts/abbey/corpus > "$LOG_DIR/contracts.log" 2>&1 || { cat "$LOG_DIR/contracts.log"; exit 1; }
cat "$LOG_DIR/contracts.log"
lock=$(python3 -c 'import json;print(json.load(open("contracts/abbey/abbey-contracts.lock.json"))["aggregate_digest"])')
grep -q "digest=$lock" "$LOG_DIR/contracts.log" || { echo "FAIL: corpus digest does not match the lock ($lock)"; exit 1; }
echo "ok: lock digest $lock"

stage "claims sync"
python3 tools/check_claims.py

stage "rust route reader (oracle)"
sh tools/stages/rust_oracle.sh "$PWD/zig-out/bin/abbey-zig"

stage "rust daemon client (wire compat against the Zig abbeyd)"
sh tools/stages/rust_daemon_client.sh "$PWD/zig-out/bin/abbeyd-zig" "$PWD/zig-out/bin/abbey-zig"

stage "e2e with a real abi binary"
sh tools/stages/e2e_abi.sh "$PWD/zig-out/bin/abbey-zig"

stage "size guard (entry points <= 200, others <= 1000)"
bad=0
for f in $(find src -name '*.zig' | sort); do
  n=$(wc -l < "$f" | tr -d ' ')
  case "$(basename "$f")" in main.zig|abbeyd.zig) if [ "$n" -gt 200 ]; then echo "FAIL $f: $n lines (max 200)"; bad=1; fi ;; esac
  if [ "$n" -gt 1000 ]; then echo "FAIL $f: $n lines (max 1000)"; bad=1; fi
done
[ "$bad" -eq 0 ] || exit 1
echo ok

echo "check.sh: OK"
