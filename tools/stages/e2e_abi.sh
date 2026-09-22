#!/bin/sh
# End-to-end P1 criteria against a real `abi` built from ../abi:
# doctor, ask, print, commit, learn status with ABBEY_BACKEND=abi.
# HOME is a temp dir (abi persists to ~/.abi); state and config are temp.
# SKIPs loudly when ABBEY_ZIG_E2E_ABI is unset.
set -eu
ZIG_BIN="$1"
ABI="${ABBEY_ZIG_E2E_ABI:-}"
if [ -z "$ABI" ] || [ ! -x "$ABI" ]; then
  echo "SKIP: e2e with a real abi binary: set ABBEY_ZIG_E2E_ABI to an abi built from ../abi"
  exit 0
fi
T=$(mktemp -d /private/tmp/abbey-zig-e2e.XXXXXX)
trap 'rm -rf "$T"' EXIT
mkdir -p "$T/home" "$T/repo"
export HOME="$T/home" ABBEY_ZIG_STATE_DIR="$T/state" ABBEY_ZIG_CONFIG="$T/none.toml" ABBEY_BACKEND=abi ABBEY_ABI_BIN="$ABI"
cd "$T/repo"
git init -q -b main && git -c user.name=t -c user.email=t@t commit -q --allow-empty -m init
check_log() { # $1 log; fail on a leak report from the Debug allocator
  if grep -qE "leaked .* allocated at|SafeAllocator leaked" "$1"; then cat "$1"; echo "FAIL: leak report"; exit 1; fi
}
run() { # $1 name, rest = args
  name="$1"; shift
  if ! "$ZIG_BIN" "$@" > "$T/$name.out" 2> "$T/$name.err"; then cat "$T/$name.out" "$T/$name.err"; echo "FAIL: $name exit $?"; exit 1; fi
  check_log "$T/$name.err"
  echo "ok: $name (exit 0)"
}
run doctor doctor
grep -q "backend:   abi (from env)" "$T/doctor.out" || { cat "$T/doctor.out"; echo "FAIL: doctor backend"; exit 1; }
grep -q "agent:     $ABI" "$T/doctor.out" || { echo "FAIL: doctor agent path"; exit 1; }
run ask ask "explain the wdbx store"
grep -q "^Abbey: " "$T/ask.out" || { cat "$T/ask.out"; echo "FAIL: ask output lacks the persona contract"; exit 1; }
[ "$(wc -l < "$T/state/route.jsonl" | tr -d ' ')" -eq 1 ] || { echo "FAIL: ask must append exactly one route record"; exit 1; }
run print print "hello there"
grep -q "provider=local" "$T/print.out" || { cat "$T/print.out"; echo "FAIL: print output"; exit 1; }
[ "$(wc -l < "$T/state/route.jsonl" | tr -d ' ')" -eq 1 ] || { echo "FAIL: print must not touch route.jsonl"; exit 1; }
printf 'hello\n' > hello.txt && git add hello.txt
run commit commit
grep -q "Write a concise conventional commit message" "$T/commit.out" || { cat "$T/commit.out"; echo "FAIL: commit prompt did not reach abi"; exit 1; }
[ "$(wc -l < "$T/state/route.jsonl" | tr -d ' ')" -eq 1 ] || { echo "FAIL: commit must not touch route.jsonl"; exit 1; }
run learn-status learn status
grep -q "activity " "$T/learn-status.out" || { cat "$T/learn-status.out"; echo "FAIL: learn status"; exit 1; }
[ -d "$T/home/.abi" ] && echo "ok: abi state landed in the temp HOME ($T/home/.abi)"
echo "ok: e2e passed with $ABI"
