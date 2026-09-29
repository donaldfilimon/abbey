#!/bin/sh
# Real-terminal smoke for `abbey-zig tui` under a pseudo-terminal (BSD or
# util-linux `script`): the TUI enters and leaves the alternate screen, a
# keypress quits it, and SIGTERM stops it with 130. In both runs `stty -g`
# inside the same pty must read the same before and after, which proves
# the termios restore on a real tty. Uses a temp HOME/state; runs no agent.
set -eu
BIN="$1"
# Compare two `stty -g` snapshots. PENDIN (lflag 0x20000000) is masked: the
# BSD kernel sets it itself whenever TCSANOW switches back to canonical mode
# (retype pending input) and clears it on the next read, so it is not user
# configuration. Every other flag and control character must match exactly.
same_termios() {
  python3 - "$1" "$2" <<'PY'
import sys
def load(p):
    f = dict(kv.split('=', 1) for kv in open(p).read().strip().split(':')[1:])
    f['lflag'] = format(int(f['lflag'], 16) & ~0x20000000, 'x')
    return f
sys.exit(0 if load(sys.argv[1]) == load(sys.argv[2]) else 1)
PY
}
command -v timeout > /dev/null 2>&1 || { echo "SKIP: no timeout(1) to bound the pty run (unmeasured)"; exit 0; }
command -v script > /dev/null 2>&1 || { echo "SKIP: no script(1) to allocate a pty (unmeasured)"; exit 0; }
case "$(uname -s)" in Darwin) ;; *) echo "SKIP: pty stage is written for BSD script(1) (unmeasured)"; exit 0 ;; esac
W=$(mktemp -d /private/tmp/abz-tui-pty.XXXXXX)
trap 'rm -rf "$W"' EXIT
mkdir -p "$W/home"
export HOME="$W/home" ABBEY_ZIG_STATE_DIR="$W/state" ABBEY_ZIG_CONFIG="$W/none.toml" ABBEY_BACKEND=abi PATH=/usr/bin:/bin
unset ABBEY_ZIG_PERSONAL_STATE_DIR || true

# 1: quit with 'q' after Ctrl-B.
(sleep 1; printf '\002'; sleep 0.5; printf 'q') | timeout 30 script -q "$W/ts1" sh -c "stty -g > '$W/before1'; '$BIN' tui; echo \$? > '$W/code1'; stty -g > '$W/after1'" > /dev/null 2>&1 || true
[ -f "$W/code1" ] || { echo "FAIL: tui did not exit after q (timed out)"; exit 1; }
[ "$(cat "$W/code1")" = 0 ] || { echo "FAIL: tui exit $(cat "$W/code1") after q"; exit 1; }
same_termios "$W/before1" "$W/after1" || { echo "FAIL: termios differs after quit"; exit 1; }
python3 - "$W/ts1" <<'PY'
import sys
d = open(sys.argv[1], 'rb').read()
assert d.count(b'\x1b[?1049h') == 1 and d.count(b'\x1b[?1049l') == 1, 'alternate screen enter/leave'
assert d.rstrip().endswith(b'\x1b[?1049l') or b'\x1b[?1049l' in d[-64:], 'leave is the last thing written'
assert b'backend -> ' in d or b'backend: no other executor' in d, 'Ctrl-B reached the state machine'
PY

# 2: SIGTERM while the TUI holds the terminal.
: | timeout 30 script -q "$W/ts2" sh -c "stty -g > '$W/before2'; '$BIN' tui < /dev/tty > /dev/tty & p=\$!; sleep 1; kill -TERM \$p; wait \$p; echo \$? > '$W/code2'; stty -g > '$W/after2'" > /dev/null 2>&1 || true
[ -f "$W/code2" ] || { echo "FAIL: tui did not exit after SIGTERM (timed out)"; exit 1; }
[ "$(cat "$W/code2")" = 130 ] || { echo "FAIL: tui exit $(cat "$W/code2") after SIGTERM (want 130)"; exit 1; }
same_termios "$W/before2" "$W/after2" || { echo "FAIL: termios differs after SIGTERM"; exit 1; }
echo "ok: tui in a real pty: q exits 0, SIGTERM exits 130, termios equal before and after both (kernel PENDIN masked)"
