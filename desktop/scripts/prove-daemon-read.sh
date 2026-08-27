#!/bin/sh
# Start an owner-only scratch abbeyd and drive the shipped desktop backend
# reads against it. The desktop crate cannot set_var (unsafe_code denied), so
# this process exports the bearer/socket/state *before* cargo test starts.
#
# Not a WebView driver. GUI absence is explicit in the log.
set -eu

cd "$(dirname "$0")/.."
DESKTOP_ROOT="$(pwd)"
REPO_ROOT="$(cd .. && pwd)"

if [ "$(uname -s)" != "Darwin" ] && [ "$(uname -s)" != "Linux" ]; then
  echo "prove-daemon-read: Unix-only (no named-pipe transport)"
  exit 1
fi

echo "GUI limit: no WebView is opened; process-level desktop backend reads against a real abbeyd are the accepted bar."

echo "== build abbeyd =="
(cd "$REPO_ROOT" && cargo build --quiet --bin abbeyd)
ABBEYD="$REPO_ROOT/target/debug/abbeyd"
test -x "$ABBEYD"

SCRATCH="$(mktemp -d /tmp/abbey-desktop-live.XXXXXX)"
chmod 700 "$SCRATCH"
SOCKET="$SCRATCH/abbeyd.sock"
PROVIDER="$SCRATCH/abi-provider"
BEARER="abbey-desktop-live-daemon-proof-token-0001"

cat > "$PROVIDER" <<'EOF'
#!/bin/sh
printf 'desktop-live-provider-output\n'
EOF
chmod 700 "$PROVIDER"

cleanup() {
  if [ -n "${DAEMON_PID-}" ]; then
    kill "$DAEMON_PID" 2>/dev/null || true
    wait "$DAEMON_PID" 2>/dev/null || true
  fi
  rm -rf "$SCRATCH"
}
trap cleanup EXIT INT TERM

echo "== start scratch abbeyd =="
ABBEY_STATE_DIR="$SCRATCH" \
  ABBEY_CONFIG="$SCRATCH/config.toml" \
  ABBEYD_SOCKET_PATH="$SOCKET" \
  ABBEYD_BEARER_TOKEN="$BEARER" \
  ABBEY_MEMORY_BACKEND=sqlite \
  ABBEY_ABI_BIN="$PROVIDER" \
  "$ABBEYD" >"$SCRATCH/daemon.stdout" 2>"$SCRATCH/daemon.stderr" &
DAEMON_PID=$!

i=0
while [ ! -S "$SOCKET" ]; do
  i=$((i + 1))
  if [ "$i" -gt 100 ]; then
    echo "abbeyd socket was not created" >&2
    cat "$SCRATCH/daemon.stderr" >&2 || true
    exit 1
  fi
  sleep 0.05
done
echo "abbeyd ready pid=$DAEMON_PID socket=$SOCKET"

echo "== cargo test live_daemon_desktop_reads_status_and_run_lifecycle =="
LOG="$SCRATCH/cargo-test.log"
set +e
ABBEY_STATE_DIR="$SCRATCH" \
  ABBEY_CONFIG="$SCRATCH/config.toml" \
  ABBEYD_SOCKET_PATH="$SOCKET" \
  ABBEYD_BEARER_TOKEN="$BEARER" \
  ABBEY_DESKTOP_LIVE_DAEMON=1 \
  cargo test --locked -p abbey-desktop live_daemon_desktop_reads -- --nocapture \
  >"$LOG" 2>&1
status=$?
set -e
cat "$LOG"
if [ "$status" -ne 0 ]; then
  echo "live daemon desktop test failed" >&2
  exit "$status"
fi
if ! grep -q 'running 1 test' "$LOG" || ! grep -q 'test result: ok. 1 passed' "$LOG"; then
  echo "live daemon desktop test did not run (vacuous cargo filter)" >&2
  exit 1
fi

echo "prove-daemon-read: OK"
