#!/bin/sh
# Rust oracle: route.jsonl written by the Zig binary must be read by the Rust
# `abbey routes` reader exactly as the Zig reader formats it. Read-only use of
# an already-built Rust binary with temp HOME/state/config; SKIPs loudly when
# the Rust binary is absent.
set -eu
ZIG_BIN="$1"
RUST_BIN="${ABBEY_ZIG_RUST_ORACLE:-$HOME/.local/bin/abbey}"
if [ ! -x "$RUST_BIN" ] || ! "$RUST_BIN" --version 2>/dev/null | grep -q '^abbey '; then
  echo "SKIP: rust route reader: no Rust abbey at $RUST_BIN (set ABBEY_ZIG_RUST_ORACLE)"
  exit 0
fi
T=$(mktemp -d /private/tmp/abbey-zig-oracle.XXXXXX)
trap 'rm -rf "$T"' EXIT
mkdir -p "$T/home" "$T/work"
printf '#!/bin/sh\nprintf "ARGV:"; for a in "$@"; do printf "[%%s]" "$a"; done; printf "\\n"\n' > "$T/stub-abi"
chmod 700 "$T/stub-abi"
cd "$T/work"
for prompt in "fix the compile error in main" "describe this screenshot" "refactor this UI screenshot layout in rust" "hmm what about that" 'quote " backslash \ tab	done'; do
  HOME="$T/home" ABBEY_ZIG_STATE_DIR="$T/state" ABBEY_ZIG_CONFIG="$T/none.toml" ABBEY_BACKEND=abi ABBEY_ABI_BIN="$T/stub-abi" \
    "$ZIG_BIN" ask "$prompt" > "$T/ask.log" 2>&1 || { cat "$T/ask.log"; echo "FAIL: zig ask"; exit 1; }
done
HOME="$T/home" ABBEY_ZIG_STATE_DIR="$T/state" ABBEY_ZIG_CONFIG="$T/none.toml" "$ZIG_BIN" routes 50 > "$T/zig-routes.txt"
HOME="$T/home" ABBEY_STATE_DIR="$T/state" ABBEY_CONFIG="$T/none.toml" "$RUST_BIN" routes 50 > "$T/rust-routes.txt" 2> "$T/rust-err.txt" || { cat "$T/rust-err.txt"; echo "FAIL: rust reader"; exit 1; }
n=$(wc -l < "$T/rust-routes.txt" | tr -d ' ')
[ "$n" -eq 5 ] || { cat "$T/rust-routes.txt"; echo "FAIL: rust reader saw $n of 5 records"; exit 1; }
diff "$T/zig-routes.txt" "$T/rust-routes.txt" || { echo "FAIL: Rust and Zig readers disagree"; exit 1; }
echo "ok: $("$RUST_BIN" --version) read all 5 Zig-written records identically"
