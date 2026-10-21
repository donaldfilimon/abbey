#!/bin/sh
# Start an owner-only scratch abbeyd and drive the shipped desktop backend
# reads against it. The desktop crate cannot set_var (unsafe_code denied), so
# this process exports the bearer/socket/state *before* cargo test starts.
#
# Not a WebView driver. GUI absence is explicit in the log.
set -eu

cd "$(dirname "$0")/.."
DESKTOP_ROOT="$(pwd)"
ABBEY_PYTHON="${ABBEY_PYTHON:-python3}"
REPO_ROOT="$(cd .. && pwd)"

if [ "$(uname -s)" != "Darwin" ] && [ "$(uname -s)" != "Linux" ]; then
  echo "prove-daemon-read: Unix-only (no named-pipe transport)"
  exit 1
fi

echo "GUI limit: no WebView is opened; process-level desktop backend reads against a real abbeyd are the accepted bar."

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

# Cargo emits actual executable paths, including configured target triples.
# A target_directory base alone can select stale host-profile artifacts.
echo "== build abbeyd and abbey =="
BUILD_JSON="$SCRATCH/build-artifacts.jsonl"
if (cd "$REPO_ROOT" && cargo build --quiet --locked --message-format=json --bin abbeyd --bin abbey) >"$BUILD_JSON"; then
  :
else
  status=$?
  cat "$BUILD_JSON" >&2
  exit "$status"
fi
artifact_path() {
  python3 -c '
import json,sys
from pathlib import Path
rows = [json.loads(line) for line in Path(sys.argv[1]).read_text().splitlines()]
paths = [row["executable"] for row in rows
         if row.get("reason") == "compiler-artifact"
         and row.get("target", {}).get("name") == sys.argv[2]
         and "bin" in row.get("target", {}).get("kind", [])
         and row.get("executable")]
if len(paths) != 1 or not isinstance(paths[0], str) or not Path(paths[0]).is_absolute():
    raise SystemExit("expected exactly one absolute built executable for " + sys.argv[2])
print(paths[0])
' "$BUILD_JSON" "$1"
}
ABBEYD="$(artifact_path abbeyd)"
ABBEY="$(artifact_path abbey)"
test -x "$ABBEYD"
test -x "$ABBEY"

# Parent-process ABI/model/bearer-file leftovers must not change negotiated
# capabilities. Unset rather than `env -u` so macOS /bin/env is enough.
unset ABBEYD_BEARER_TOKEN_FILE
unset ABBEY_PERSONAL_BEARER_TOKEN
unset ABBEY_PERSONAL_BEARER_TOKEN_FILE
unset ABBEY_MODEL_MANIFEST_DIR
unset ABBEY_MODEL_RUNTIME_CONFIG
unset ABI_MODELS_DIR
unset ABBEY_PERSONAL_ABI_BIN

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

echo "== seed synthetic authorized conversation history =="
ABBEYD_SOCKET_PATH="$SOCKET" \
  ABBEYD_BEARER_TOKEN="$BEARER" \
  ABBEY_DESKTOP_CONVERSATION_FIXTURE=1 \
  "$ABBEY_PYTHON" scripts/seed-conversation-fixture.py
touch "$SCRATCH/conversation-seed-observed"

echo "== seed sanitized memory summary =="
ABBEY_STATE_DIR="$SCRATCH" \
  ABBEY_CONFIG="$SCRATCH/config.toml" \
  ABBEY_MEMORY_BACKEND=sqlite \
  "$ABBEY" memory put "desktop-memory-proof canary" \
    --payload RAW_PAYLOAD_CANARY \
    --provenance RAW_PROVENANCE_CANARY \
    --source-ref /private/source/canary \
    >/dev/null

echo "== cargo test live_daemon_desktop_reads =="
LOG="$SCRATCH/cargo-test.log"
set +e
ABBEY_STATE_DIR="$SCRATCH" \
  ABBEY_CONFIG="$SCRATCH/config.toml" \
  ABBEYD_SOCKET_PATH="$SOCKET" \
  ABBEYD_BEARER_TOKEN="$BEARER" \
  ABBEY_MEMORY_BACKEND=sqlite \
  ABBEY_DESKTOP_LIVE_DAEMON=1 \
  cargo test --locked -p abbey-desktop live_daemon_desktop -- --nocapture \
  >"$LOG" 2>&1
status=$?
set -e
cat "$LOG"
if [ "$status" -ne 0 ]; then
  echo "live daemon desktop test failed" >&2
  exit "$status"
fi
# The live-test count is read from the log rather than restated here: a
# hardcoded count quietly goes stale the moment a live test is added, and a
# stale count is how a vacuous cargo filter passes unnoticed. The count must
# be at least one — a filter that matched nothing is not a proof.
RUNNING="$(sed -n 's/^running \([0-9][0-9]*\) tests$/\1/p' "$LOG" | head -1)"
PASSED="$(sed -n 's/^test result: ok\. \([0-9][0-9]*\) passed.*/\1/p' "$LOG" | head -1)"
if [ -z "$RUNNING" ] || [ -z "$PASSED" ] || [ "$RUNNING" -lt 1 ] || [ "$RUNNING" != "$PASSED" ]; then
  echo "live daemon desktop tests did not all run (vacuous cargo filter)" >&2
  exit 1
fi
echo "live daemon desktop tests ran: $RUNNING"

echo "prove-daemon-read: OK"
