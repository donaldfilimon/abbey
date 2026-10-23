#!/usr/bin/env python3
"""Native PTY/TCP acceptance for the narrow governed scratch-file demonstration.

Uses already-built native binaries. Accounts/sessions are explicitly seeded test
fixtures: this does not exercise OIDC login, signup, or an external identity
provider. Metadata/JWKS are served by a loopback fixture only. All private state,
keys, sessions, journals and file effects live in an owner-only temporary tree.
No live Abbey or ABI store is accessed. POSIX PTYs are required.
"""
import argparse
import base64
import hashlib
import http.server
import json
import os
from pathlib import Path
import pty
import re
import secrets
import select
import signal
import shutil
import socket
import sqlite3
import subprocess
import sys
import tempfile
import threading
import time
import urllib.error
import urllib.request
import uuid

from governed_response_loss import ResponseLossProxy

WAIT = 45
LIMIT = 1024 * 1024


def require(condition, message):
    if not condition:
        raise AssertionError(message)


def private_json(path, value):
    with open(path, "x", encoding="utf-8") as stream:
        os.chmod(path, 0o600)
        json.dump(value, stream)


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":")).encode()


def digest(value):
    return base64.urlsafe_b64encode(hashlib.sha256(value.encode()).digest()).decode().rstrip("=")


def free_port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def stop(process):
    if process.poll() is None:
        try:
            os.killpg(process.pid, signal.SIGTERM)
        except ProcessLookupError:
            pass
        try:
            process.wait(timeout=3)
        except subprocess.TimeoutExpired:
            os.killpg(process.pid, signal.SIGKILL)
            process.wait(timeout=3)


class Terminal:
    def __init__(self, command, env, cwd):
        self.master, slave = pty.openpty()
        self.process = subprocess.Popen(command, stdin=slave, stdout=slave,
                                        stderr=slave, env=env, cwd=cwd,
                                        start_new_session=True)
        os.close(slave)
        self.output = bytearray()

    def until(self, pattern, timeout=WAIT, start=0):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            match = re.search(pattern, self.output.decode("utf-8", "replace")[start:])
            if match:
                return match
            if select.select([self.master], [], [], .1)[0]:
                try:
                    chunk = os.read(self.master, 65536)
                except OSError:
                    chunk = b""
                self.output.extend(chunk)
                require(len(self.output) <= LIMIT, "PTY output exceeded bounded capture")
                if chunk:
                    continue
            require(self.process.poll() is None,
                    "native PTY child exited before expected checkpoint")
        raise AssertionError("native PTY checkpoint timed out")

    def drain(self):
        while select.select([self.master], [], [], 0)[0]:
            try:
                chunk = os.read(self.master, 65536)
            except OSError:
                break
            if not chunk:
                break
            self.output.extend(chunk)
            require(len(self.output) <= LIMIT, "PTY output exceeded bounded capture")

    def write(self, line):
        os.write(self.master, line.encode() + b"\n")

    def close(self):
        stop(self.process)
        os.close(self.master)


class Metadata(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        origin = f"http://127.0.0.1:{self.server.server_port}"
        if self.path == "/.well-known/openid-configuration":
            value = {"issuer": origin, "authorization_endpoint": origin + "/authorize",
                     "token_endpoint": origin + "/token", "jwks_uri": origin + "/jwks",
                     "response_types_supported": ["code"],
                     "id_token_signing_alg_values_supported": ["EdDSA"]}
        elif self.path == "/jwks":
            # Only discovery is exercised; no token signed with this key is accepted.
            value = {"keys": [{"kty": "OKP", "crv": "Ed25519", "kid": "fixture",
                               "use": "sig", "alg": "EdDSA",
                               "x": base64.urlsafe_b64encode(bytes([1]) * 32).decode().rstrip("=")}]}
        else:
            self.send_error(404)
            return
        data = canonical(value)
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def log_message(self, *_):
        pass


def run(args):
    require(os.name == "posix", "this acceptance script requires POSIX native PTYs")
    binaries = {name: str(Path(getattr(args, name)).resolve())
                for name in ("control_plane", "enroll", "relay_job", "worker")}
    for binary in binaries.values():
        require(os.path.isfile(binary) and os.access(binary, os.X_OK),
                "every binary argument must identify an existing executable")
    processes, terminals, logs = [], [], []
    metadata = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Metadata)
    thread = threading.Thread(target=metadata.serve_forever, daemon=True)
    thread.start()
    temporary = tempfile.mkdtemp(prefix="abbey-governed-native-")
    failed = True
    proxy = None
    try:
        root = Path(temporary)
        root.chmod(0o700)
        env = dict(os.environ, TERM="xterm-256color",
                   ABBEY_STATE_DIR=str(root / "abbey-unused"),
                   ABBEY_PERSONAL_STATE_DIR=str(root / "personal-unused"),
                   ABI_WDBX_PATH=":memory:", ABI_WDBX_PERSIST="0")
        backend_port = free_port()
        proxy = ResponseLossProxy(backend_port)
        origin = f"http://127.0.0.1:{proxy.server_port}"
        database = root / "authority.sqlite"
        config = root / "server.json"
        private_json(config, {"listen": f"127.0.0.1:{backend_port}",
                              "database": str(database), "oidc": {
                                  "issuer": f"http://127.0.0.1:{metadata.server_port}",
                                  "client_id": "native-fixture", "client_secret": None,
                                  "origin": origin, "loopback_development": True}})

        def command(binary, *arguments):
            result = subprocess.run([binary, *map(str, arguments)], cwd=root, env=env,
                                    stdin=subprocess.DEVNULL, capture_output=True,
                                    timeout=WAIT)
            require(len(result.stdout) + len(result.stderr) <= LIMIT,
                    "native command output exceeded bound")
            require(result.returncode == 0, "native command refused: " + Path(binary).name
                    + " " + str(arguments[0] if arguments else ""))
            return result.stdout

        command(binaries["control_plane"], "init", database, origin)
        account, tenant, token, csrf = (str(uuid.uuid4()), str(uuid.uuid4()),
                                        secrets.token_urlsafe(32), secrets.token_urlsafe(32))
        other_token, other_csrf = secrets.token_urlsafe(32), secrets.token_urlsafe(32)
        with sqlite3.connect(database) as db:
            for aid, tid, session, anti_csrf in [
                    (account, tenant, token, csrf),
                    (str(uuid.uuid4()), str(uuid.uuid4()), other_token, other_csrf)]:
                db.execute("INSERT INTO accounts VALUES(?,?,?,?)",
                           (aid, tid, "fixture:not-oidc-login", aid))
                db.execute("INSERT INTO sessions VALUES(?,?,?,?)",
                           (digest(session), digest(anti_csrf), aid, int(time.time()) + 3600))
        opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))

        def request(path, body=None, alternate=False, authenticated=True):
            headers = {}
            if authenticated:
                headers["Cookie"] = "abi_session=" + (other_token if alternate else token)
            if body is not None:
                headers.update({"Content-Type": "application/json", "Origin": origin,
                                "X-CSRF-Token": other_csrf if alternate else csrf})
            payload = None if body is None else canonical(body)
            req = urllib.request.Request(origin + path, data=payload, headers=headers)
            with opener.open(req, timeout=5) as response:
                data = response.read(LIMIT + 1)
                require(len(data) <= LIMIT, "HTTP response exceeded bound")
                return json.loads(data)

        def start_server():
            log = open(root / f"server-{len(logs)}.log", "xb")
            os.chmod(log.name, 0o600)
            logs.append(log)
            process = subprocess.Popen([binaries["control_plane"], "serve", str(config)],
                                       stdin=subprocess.DEVNULL, stdout=log, stderr=log,
                                       cwd=root, env=env, start_new_session=True)
            processes.append(process)
            deadline = time.monotonic() + WAIT
            while time.monotonic() < deadline:
                require(process.poll() is None, "control-plane startup failed")
                try:
                    request("/v1/authority", authenticated=False)
                    return process
                except (OSError, urllib.error.URLError):
                    time.sleep(.1)
            raise AssertionError("control-plane readiness timed out")

        server = start_server()
        node = root / "node"
        enrollment = Terminal([binaries["enroll"], origin, str(node), "--tools"], env, root)
        terminals.append(enrollment)
        code = enrollment.until(r"pair with code:\s*([A-Za-z0-9_-]+)").group(1)
        claim = request("/v1/enrollments/claim", {"user_code": code})
        require(claim["account_id"] == account and claim["tenant_id"] == tenant,
                "enrollment escaped fixture account scope")
        enrollment.until(r"Type the account ID to approve")
        enrollment.write(account)
        enrollment.until(r"enrolled and possession verified")
        require(enrollment.process.wait(timeout=10) == 0, "native enrollment failed")
        enrollment.close()
        terminals.remove(enrollment)

        refused = subprocess.run([binaries["worker"], str(node), "--initialize",
                                  "--loopback-development"], env=env, cwd=root,
                                 stdin=subprocess.DEVNULL, capture_output=True, timeout=10)
        require(refused.returncode != 0 and b"interactive" in refused.stderr,
                "nonterminal worker did not refuse local approval authority")
        require(not (node / "tool-worker-control.log").exists(),
                "nonterminal worker created execution state")

        proposal, client = root / "proposal.json", root / "client"
        canary = "native-private-canary-" + secrets.token_hex(12)
        private_json(proposal, {"protocol": "abi.governed-tool.v1",
                                "tool": "create_scratch", "revision": "scratch-create-v1",
                                "parameters": {"content": canary}})
        command(binaries["relay_job"], "prepare-tool", node, proposal, client,
                "scratch-create-v1")
        submission = json.loads((client / "submission.json").read_text())
        binding = submission["submission"]["binding"]
        job = binding["job_id"]
        request("/v2/jobs", submission)
        worker = Terminal([binaries["worker"], str(node), "--initialize",
                           "--loopback-development"], env, root)
        terminals.append(worker)
        approval = worker.until(r"Type exactly: approve (sha256:[0-9a-f]{64})").group(1)
        effect = node / "governed-scratch" / f"effect-{job}.txt"
        require(not effect.exists(), "file effect happened before local exact approval")
        control_before = (node / "tool-worker-control.log").read_bytes()
        with sqlite3.connect(node / "tool-approvals.sqlite") as db:
            require(db.execute("SELECT COUNT(*) FROM governed_tool_intents").fetchone()[0] == 0,
                    "execution intent exists before local approval")
        worker.write("approve " + approval)
        worker.until(r"outcome=Recorded")

        publication_restarts = 0

        def await_delivery():
            nonlocal worker, publication_restarts
            deadline = time.monotonic() + WAIT
            last = {}
            while time.monotonic() < deadline:
                worker.drain()  # A completed stdout prefix must not block publication on PTY capacity.
                if worker.process.poll() is not None and "publication" in proxy.evidence:
                    require(publication_restarts == 0, "worker repeatedly exited after publication loss")
                    publication_restarts += 1
                    worker.close()
                    terminals.remove(worker)
                    worker = Terminal([binaries["worker"], str(node), "--loopback-development"], env, root)
                    terminals.append(worker)
                    worker.until(r"demonstration ready")
                require(worker.process.poll() is None, "native worker exited during delivery: "
                        + json.dumps(last, sort_keys=True))
                try:
                    page = request(f"/v2/jobs/{job}/delivery")
                    last = {"receipt_state": page.get("receipt", {}).get("state"),
                            "packets": len(page.get("packets", [])),
                            "manifest": bool(page.get("manifest")),
                            "pty_bytes": len(worker.output)}
                    if page.get("manifest") and any(packet.get("kind") == "terminal"
                                                    for packet in page.get("packets", [])):
                        require(b"Type exactly: approve" not in worker.output or
                                publication_restarts == 0,
                                "publication recovery requested new local approval")
                        return page
                except urllib.error.HTTPError as error:
                    last = {"http_status": error.code, "pty_bytes": len(worker.output)}
                except OSError as error:
                    last = {"network_error": type(error).__name__}
                time.sleep(.2)
            worker.drain()
            with sqlite3.connect(node / "tool-approvals.sqlite") as db:
                last["local_intents"] = db.execute(
                    "SELECT state,COUNT(*) FROM governed_tool_intents GROUP BY state").fetchall()
            # Report only state labels; never print approval previews or raw parameters.
            transcript = worker.output.decode("utf-8", "replace")
            last["worker_outcomes"] = re.findall(r"outcome=(Recorded|Pending|Unresolved)", transcript)
            last["worker_refused"] = "Governed worker refused:" in transcript
            raise AssertionError("encrypted terminal delivery timed out: " + json.dumps(last, sort_keys=True))

        page = await_delivery()
        require(effect.read_text() == canary, "actual scratch effect differs from approval")
        require(len(list((node / "governed-scratch").iterdir())) == 1,
                "unexpected additional scratch effects")
        require((node / "tool-worker-control.log").read_bytes() != control_before,
                "approved job was not durably admitted")
        observed = (effect.stat().st_ino, effect.stat().st_mtime_ns, effect.read_bytes())
        first_packets = canonical(page["packets"])
        first_manifest = canonical(page["manifest"])
        require("publication" in proxy.evidence,
                "no accepted publication response was lost on the real TCP connection")
        require(proxy.evidence["publication"]["packet_sha256"] in
                [hashlib.sha256(canonical(packet)).hexdigest() for packet in page["packets"]],
                "recovery changed the accepted packet whose response was lost")
        page_file = root / "delivery.json"
        private_json(page_file, page)
        command(binaries["relay_job"], "import", client, page_file)
        output = command(binaries["relay_job"], "open-durable", client)
        require(b"Succeeded" in output or b'"succeeded"' in output.lower(),
                "client did not reconstruct successful durable output")
        # Exercise a separate encrypted proposal and the actual local DENY path.
        denied_proposal, denied_client = root / "denied-proposal.json", root / "denied-client"
        denied_canary = "native-denied-private-canary-" + secrets.token_hex(12)
        private_json(denied_proposal, {"protocol": "abi.governed-tool.v1",
                                      "tool": "create_scratch", "revision": "scratch-create-v1",
                                      "parameters": {"content": denied_canary}})
        command(binaries["relay_job"], "prepare-tool", node, denied_proposal,
                denied_client, "scratch-create-v1")
        denied_submission = json.loads((denied_client / "submission.json").read_text())
        denied_job = denied_submission["submission"]["binding"]["job_id"]
        require(denied_job != job, "denial proposal reused the approved job identity")
        worker.drain()
        prompt_cursor = len(worker.output.decode("utf-8", "replace"))
        request("/v2/jobs", denied_submission)
        denied_approval = worker.until(r"Type exactly: approve (sha256:[0-9a-f]{64})",
                                       start=prompt_cursor).group(1)
        require(denied_approval != approval, "denial matched the prior approval preview")
        denied_effect = node / "governed-scratch" / f"effect-{denied_job}.txt"

        def local_denial_record():
            with sqlite3.connect(node / "tool-approvals.sqlite") as db:
                rows = db.execute("SELECT a.state,p.preview_json FROM tool_approvals a "
                                  "JOIN local_governed_approvals p ON a.call_id=p.call_id").fetchall()
            matches = [(state, json.loads(preview)) for state, preview in rows
                       if json.loads(preview)["request"]["attempt_id"] == denied_job]
            require(len(matches) == 1, "new denial prompt lacks one correlated durable preview")
            return matches[0]

        denial_state, denial_preview = local_denial_record()
        require(denial_state == "pending" and
                denial_preview["parameters"] == {"content": denied_canary} and
                denial_preview["account_id"] == account,
                "denial prompt does not represent the exact second proposal")
        require(not denied_effect.exists(), "second effect happened before local denial")
        before_denial = (node / "tool-worker-control.log").read_bytes()
        worker.write("deny " + denied_approval)
        deadline = time.monotonic() + WAIT
        denial_metadata = None
        while time.monotonic() < deadline:
            worker.drain()
            denial_state, _ = local_denial_record()
            denial_view = request(f"/v1/jobs/{denied_job}")
            denial_metadata = denial_view.get("receipt", {}).get("state")
            if denial_state == "denied" and denial_metadata in ("unresolved", "cancelled", "expired", "failed"):
                break
            require(worker.process.poll() is None,
                    "worker exited before acknowledging durable local denial")
            time.sleep(.1)
        require(denial_state == "denied" and denial_metadata in
                ("unresolved", "cancelled", "expired", "failed"),
                "local denial was not acknowledged with safe owning-account metadata")
        require(not denied_effect.exists(), "locally denied proposal performed an effect")
        require((node / "tool-worker-control.log").read_bytes() == before_denial,
                "locally denied proposal acquired worker admission")
        with sqlite3.connect(node / "tool-approvals.sqlite") as db:
            require(db.execute("SELECT COUNT(*) FROM governed_tool_intents WHERE call_id=?",
                               (denial_preview["request"]["call_id"],)).fetchone()[0] == 0,
                    "locally denied proposal acquired a governed intent")
        require((effect.stat().st_ino, effect.stat().st_mtime_ns, effect.read_bytes()) == observed,
                "denial changed the previously approved effect")
        require(len(list((node / "governed-scratch").iterdir())) == 1,
                "denial created an additional effect")
        # Do not upload the generated client ack: exercise redelivery after restart.
        for alternate, authenticated in [(True, True), (False, False)]:
            try:
                request(f"/v2/jobs/{job}/delivery", alternate=alternate,
                        authenticated=authenticated)
            except urllib.error.HTTPError as error:
                require(error.code in (401, 403, 404), "unexpected cross-account response")
            else:
                raise AssertionError("foreign or unauthenticated account read delivery")
        worker.close()
        terminals.remove(worker)
        stop(server)
        server = start_server()
        worker = Terminal([binaries["worker"], str(node), "--loopback-development"], env, root)
        terminals.append(worker)
        worker.until(r"demonstration ready")
        replay = await_delivery()
        require(canonical(replay["packets"]) == first_packets,
                "restart redelivery changed signed encrypted packet bytes")
        require(canonical(replay["manifest"]) == first_manifest,
                "restart redelivery changed signed manifest bytes")
        require((effect.stat().st_ino, effect.stat().st_mtime_ns, effect.read_bytes()) == observed,
                "restart repeated or changed the file effect")
        require(len(list((node / "governed-scratch").iterdir())) == 1,
                "restart created an extra effect")
        require(local_denial_record()[0] == "denied" and not denied_effect.exists(),
                "restart changed durable denial or created the refused effect")
        replay_file = root / "redelivery.json"
        private_json(replay_file, replay)
        command(binaries["relay_job"], "import", client, replay_file)
        require(command(binaries["relay_job"], "open-durable", client) == output,
                "duplicate client import changed reconstructed output")
        ack = json.loads((client / "delivery-ack.json").read_text())["ack"]
        try:
            request(f"/v2/jobs/{job}/delivery/ack", ack)
        except (OSError, urllib.error.URLError):
            pass
        else:
            raise AssertionError("accepted consumer ACK response was not lost")
        require("consumer_ack" in proxy.evidence,
                "consumer ACK was not accepted upstream before response loss")
        for _ in range(2):
            require(request(f"/v2/jobs/{job}/delivery/ack", ack) is True,
                    "duplicate consumer ACK was refused")
        stop(server)
        server = start_server()
        require(request(f"/v2/jobs/{job}/delivery/ack", ack) is True,
                "consumer ACK did not survive authority restart")
        require(command(binaries["relay_job"], "open-durable", client) == output,
                "lost ACK or restart changed imported terminal output")
        require((effect.stat().st_ino, effect.stat().st_mtime_ns, effect.read_bytes()) == observed,
                "lost ACK repeated or changed scratch effect")
        for path in root.glob("authority.sqlite*"):
            require(canary.encode() not in path.read_bytes() and
                    denied_canary.encode() not in path.read_bytes(),
                    "private plaintext parameters leaked into central persistence")
        print(json.dumps({"result": "passed", "transport": "native TCP loopback + PTY",
                          "identity": "seeded account/session fixture; OIDC login untested",
                          "native_enrollment": True, "nonterminal_refused": True,
                          "effect_count": 1, "restart_ciphertext_identical": True,
                          "restart_manifest_identical": True,
                          "response_loss_evidence": proxy.evidence,
                          "duplicate_ack_after_restart": True,
                          "publication_loss_worker_restarts": publication_restarts,
                          "client_duplicate_import_identical": True,
                          "cross_account_refused": True,
                          "local_denial_refused_effect": True,
                          "private_parameters_absent_from_central_database": True}, sort_keys=True))
        failed = False
    finally:
        for terminal in reversed(terminals):
            terminal.close()
        for process in reversed(processes):
            stop(process)
        for log in logs:
            log.close()
        if proxy is not None:
            proxy.close()
        metadata.shutdown()
        metadata.server_close()
        thread.join(timeout=3)
        if failed and args.keep_on_failure:
            print(json.dumps({"retained_private_failure_fixture": temporary,
                              "contains": "scratch keys and sessions; owner-only test data"}),
                  file=sys.stderr)
        else:
            shutil.rmtree(temporary)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("control-plane", "enroll", "relay-job", "worker"):
        parser.add_argument("--" + name, required=True, help="already-built native executable")
    parser.add_argument("--keep-on-failure", action="store_true",
                        help="retain owner-only scratch fixture and private diagnostics after failure")
    args = parser.parse_args()
    run(args)


if __name__ == "__main__":
    main()
