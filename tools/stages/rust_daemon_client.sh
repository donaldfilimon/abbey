#!/bin/sh
# Wire compatibility: the Rust `abbey daemon` client (an already-built binary,
# run read-only) must talk to the Zig abbeyd. The Rust client sends protocol
# v2 first; it only succeeds if the Zig daemon answers `unsupported_version`
# with the request_id echoed and then serves the v1 retry. The Rust client
# re-validates every route-audit page (no path, no control character, ws-
# digest, RFC 3339), so a hostile route.jsonl line is the sanitization oracle.
#
# Isolation: temp HOME, temp Zig and Rust state roots, and the Rust client's
# ABBEYD_SOCKET_PATH / ABBEYD_BEARER_TOKEN pointed at the temp socket and a
# random token. Donald's live Rust state and ~/.abi are never touched.
# SKIPs loudly when the Rust binary is absent.
set -eu
ZIG_DAEMON="$1"
ZIG_BIN="$2"
RUST_BIN="${ABBEY_ZIG_RUST_ORACLE:-$HOME/.local/bin/abbey}"
if [ ! -x "$RUST_BIN" ] || ! "$RUST_BIN" --version 2>/dev/null | grep -q '^abbey '; then
  echo "SKIP: rust daemon client: no Rust abbey at $RUST_BIN (set ABBEY_ZIG_RUST_ORACLE)"
  exit 0
fi
if ! "$RUST_BIN" daemon status --help > /dev/null 2>&1; then
  echo "SKIP: rust daemon client: $RUST_BIN has no \`daemon status\` verb"
  exit 0
fi
# Short root: Darwin's sun_path holds 104 bytes.
T=$(mktemp -d /private/tmp/abz-gate.XXXXXX)
PID=""
cleanup() {
  if [ -n "$PID" ]; then kill -TERM "$PID" 2>/dev/null || true; wait "$PID" 2>/dev/null || true; fi
  rm -rf "$T"
}
trap cleanup EXIT
mkdir -p "$T/home" "$T/state"
SOCK="$T/d/s.sock"
TOKEN=$(python3 -c 'import secrets; print(secrets.token_hex(24))')
# Record 1: hostile reason (keyed path, NBSP-separated path, HOME path, BEL),
# a real absolute cwd, a stage, and tools. Record 2: an invalid timestamp the
# daemon must drop. Record 3: ordinary.
python3 - "$T" > "$T/state/route.jsonl" <<'PY'
import json, sys
t = sys.argv[1]
rows = [
    {"ts": "2026-09-21T10:11:12Z", "cwd": t + "/secret-project", "persona": "Abbey", "role": "max", "model": "fable",
     "reason": "persona=Abbey class=Code log=/var/log/abbey.jsonl wrote\u00a0/etc/passwd in " + t + "/home/x bell\u0007",
     "confidence": 0.735, "tools": ["media", "/usr/bin/tool"], "stage": "gate"},
    {"ts": "yesterday", "cwd": "/tmp/p", "persona": "Abbey", "role": "max", "model": "fable", "reason": "dropped", "confidence": 0.5, "tools": []},
    {"ts": "2026-09-21T10:11:13Z", "cwd": "", "persona": "Aviva", "role": "gemma", "model": "local", "reason": "persona=aviva", "confidence": 1.0, "tools": []},
]
for r in rows:
    print(json.dumps(r, ensure_ascii=False))
PY
HOME="$T/home" ABBEY_ZIG_STATE_DIR="$T/state" ABBEY_ZIG_DAEMON_SOCKET_PATH="$SOCK" ABBEY_ZIG_DAEMON_BEARER_TOKEN="$TOKEN" \
  "$ZIG_DAEMON" > "$T/daemon.log" 2>&1 &
PID=$!
i=0
while [ ! -S "$SOCK" ]; do
  i=$((i + 1))
  if [ "$i" -gt 200 ]; then cat "$T/daemon.log"; echo "FAIL: Zig daemon socket never appeared"; exit 1; fi
  kill -0 "$PID" 2>/dev/null || { cat "$T/daemon.log"; echo "FAIL: Zig daemon exited"; exit 1; }
  sleep 0.05
done
mode=$(stat -f '%Lp' "$SOCK"); dmode=$(stat -f '%Lp' "$T/d")
[ "$mode" = "600" ] && [ "$dmode" = "700" ] || { echo "FAIL: socket mode $mode / dir mode $dmode (want 600 / 700)"; exit 1; }

rust() {
  HOME="$T/home" ABBEY_STATE_DIR="$T/rust-state" ABBEY_CONFIG="$T/none.toml" \
    ABBEYD_SOCKET_PATH="$SOCK" ABBEYD_BEARER_TOKEN="$1" "$RUST_BIN" daemon "$2" --json > "$T/rust-$2.json" 2> "$T/rust-$2.err"
}
for verb in status claims routes; do
  rust "$TOKEN" "$verb" || { cat "$T/rust-$verb.err"; echo "FAIL: Rust client \`daemon $verb\` against the Zig daemon"; exit 1; }
done
python3 - "$T" <<'PY'
import json, sys
t = sys.argv[1]
st = json.load(open(t + "/rust-status.json"))
p = st["payload"]
assert st["type"] == "status", st
assert (p["protocol_version"], p["schema_version"], p["state"]) == (1, 1, "ready"), p
assert p["capabilities"]["capabilities"] == ["read_status", "read_claims", "read_routes"], p
cl = json.load(open(t + "/rust-claims.json"))["payload"]
assert cl["matched"] == len(cl["claims"]) > 0, cl
rt_raw = open(t + "/rust-routes.json").read()
rt = json.loads(rt_raw)["payload"]
assert rt["returned"] == 2 and rt["limit"] == 50, rt
e = rt["entries"][0]
assert e["workspace"].startswith("ws-") and len(e["workspace"]) == 15, e
assert e["confidence_percent"] == 74, e
assert e["reason"] == "persona=Abbey class=Code [path] wrote [path] in [path] bell", e["reason"]
assert e["tools"] == ["media", "[path]"], e
assert "workspace" not in rt["entries"][1], rt["entries"][1]
for leaked in (t, "/var/log", "/etc/passwd", "/usr/bin", "\u0007", "\\u0007"):
    assert leaked not in rt_raw, leaked
print("ok: status v1/schema 1, %d claims, 2 of 3 routes (invalid timestamp dropped), no path or control character on the wire" % cl["matched"])
PY
# Prove the downgrade instead of inferring it: relay one Rust `daemon status`
# through a recording proxy socket and check the request versions it sent.
PROXY="$T/p.sock"
python3 - "$PROXY" "$SOCK" > "$T/proxy.log" 2>&1 <<'PY' &
import json, os, socket, struct, sys
proxy, target = sys.argv[1], sys.argv[2]
srv = socket.socket(socket.AF_UNIX); srv.bind(proxy); os.chmod(proxy, 0o600); srv.listen(4); srv.settimeout(20)
def frame(s):
    head = b""
    while len(head) < 4:
        chunk = s.recv(4 - len(head))
        if not chunk: return None
        head += chunk
    n = struct.unpack(">I", head)[0]; body = b""
    while len(body) < n:
        chunk = s.recv(n - len(body))
        if not chunk: return None
        body += chunk
    return head + body
seen = []
while len(seen) < 2:
    c, _ = srv.accept(); req = frame(c)
    if req is None: break
    doc = json.loads(req[4:]); seen.append(doc["version"])
    u = socket.socket(socket.AF_UNIX); u.connect(target); u.sendall(req); resp = frame(u); u.close()
    c.sendall(resp); c.close()
    code = json.loads(resp[4:])["payload"].get("code", "ok")
    print("request version", doc["version"], "->", code, flush=True)
    if code == "ok": break
PY
PROXY_PID=$!
i=0
while [ ! -S "$PROXY" ]; do i=$((i + 1)); [ "$i" -le 100 ] || { cat "$T/proxy.log"; echo "FAIL: proxy"; exit 1; }; sleep 0.05; done
HOME="$T/home" ABBEY_STATE_DIR="$T/rust-state" ABBEY_CONFIG="$T/none.toml" ABBEYD_SOCKET_PATH="$PROXY" ABBEYD_BEARER_TOKEN="$TOKEN" \
  "$RUST_BIN" daemon status > "$T/rust-proxied.txt" 2>&1 || { cat "$T/rust-proxied.txt" "$T/proxy.log"; echo "FAIL: proxied Rust client"; exit 1; }
wait "$PROXY_PID" || true
printf 'request version 2 -> unsupported_version\nrequest version 1 -> ok\n' | diff - "$T/proxy.log" \
  || { echo "FAIL: expected the Rust client's v2 attempt to be refused and its v1 retry served"; exit 1; }
echo "ok: observed the Rust client send v2 (refused: unsupported_version), then v1 (served)"
if rust "$(python3 -c 'print("a" * 48)')" status; then echo "FAIL: wrong bearer was accepted"; exit 1; fi
grep -q unauthorized "$T/rust-status.err" || { cat "$T/rust-status.err"; echo "FAIL: wrong bearer did not report unauthorized"; exit 1; }
# The Zig client agrees with the Rust one on the same daemon.
HOME="$T/home" ABBEY_ZIG_STATE_DIR="$T/state" ABBEY_ZIG_DAEMON_SOCKET_PATH="$SOCK" ABBEY_ZIG_DAEMON_BEARER_TOKEN="$TOKEN" \
  "$ZIG_BIN" daemon routes --json > "$T/zig-routes.json" 2>&1 || { cat "$T/zig-routes.json"; echo "FAIL: Zig client"; exit 1; }
python3 -c 'import json,sys; a=json.load(open(sys.argv[1])); b=json.load(open(sys.argv[2])); assert a==b, (a,b)' "$T/rust-routes.json" "$T/zig-routes.json" \
  || { echo "FAIL: Zig and Rust clients decoded different route pages"; exit 1; }
kill -TERM "$PID"
wait "$PID" || { cat "$T/daemon.log"; echo "FAIL: Zig daemon exited nonzero on SIGTERM"; exit 1; }
PID=""
[ ! -e "$SOCK" ] || { echo "FAIL: socket left behind after SIGTERM"; exit 1; }
echo "ok: $("$RUST_BIN" --version) client talked to the Zig daemon (v2 -> v1 downgrade), wrong bearer refused, socket removed on SIGTERM"
