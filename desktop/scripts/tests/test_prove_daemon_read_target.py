"""Run the shipped shell proof with owned fakes; never run Rust or a provider.

Only the shell script's bytes are copied into the disposable fixture tree.
"""

import json
import os
from pathlib import Path
import shlex
import signal
import subprocess
import sys
import tempfile
import unittest


SCRIPT = Path(__file__).resolve().parents[1] / "prove-daemon-read.sh"


FAKE_CARGO = r'''
import json
import os
from pathlib import Path
import shlex
import socket
import sys
import tempfile

root = Path(os.environ["ABBEY_PROOF_FIXTURE_ROOT"]).resolve()
events = Path(os.environ["ABBEY_PROOF_FIXTURE_EVENTS"])

def record(**event):
    with events.open("a") as stream:
        stream.write(json.dumps(event) + "\n")

tool, args = sys.argv[1], sys.argv[2:]
if tool == "mktemp":
    if len(args) != 2 or args[0] != "-d" or "abbey-desktop-live" not in args[1]:
        raise SystemExit("unexpected scratch command")
    print(tempfile.mkdtemp(prefix="script-scratch-", dir=root))
    raise SystemExit(0)

record(event="cargo", args=args, cwd=str(Path.cwd()))
if not args:
    raise SystemExit("missing cargo command")

workspace = Path.cwd().resolve()
if "--manifest-path" in args:
    workspace = Path(args[args.index("--manifest-path") + 1]).resolve().parent
configured_target = os.environ.get("CARGO_TARGET_DIR")
if configured_target:
    target = Path(configured_target)
    if not target.is_absolute():
        target = Path.cwd() / target
    target = target.resolve()
else:
    target = workspace / "target"
try:
    target.relative_to(root)
except ValueError:
    raise SystemExit("target escaped fixture")

# Metadata reports the target-dir base, not the target-platform profile path.
# Model only the two fixed fixture configuration forms used by this suite.
build_target = os.environ.get("CARGO_BUILD_TARGET")
config = root / ".cargo" / "config.toml"
if not build_target and config.is_file():
    for line in config.read_text().splitlines():
        if line.startswith("target = "):
            build_target = json.loads(line.split(" = ", 1)[1])
artifact_target = target / build_target if build_target else target
artifact_target.resolve().relative_to(root)

if args[0] == "metadata":
    print(json.dumps({"version": 1, "workspace_root": str(workspace),
                      "target_directory": str(target)}))
elif args[0] == "build":
    if workspace != root or "abbeyd" not in args or "abbey" not in args:
        raise SystemExit("unexpected build")
    for name in ("abbeyd", "abbey"):
        executable = artifact_target / "debug" / name
        executable.parent.mkdir(parents=True, exist_ok=True)
        command = [sys.executable, "-I", str(root / "artifact_stub.py"),
                   name, "active", str(executable)]
        executable.write_text("#!/bin/sh\nexec " + shlex.join(command) + ' "$@"\n')
        executable.chmod(0o700)
        if "--message-format=json" in args:
            print(json.dumps({"reason": "compiler-artifact", "target": {"name": name, "kind": ["bin"]},
                              "executable": str(executable)}))
    record(event="built", target=str(artifact_target))
elif args[0] == "test":
    if ("live_daemon_desktop" not in args
            or os.environ.get("ABBEY_DESKTOP_LIVE_DAEMON") != "1"):
        raise SystemExit("unexpected test")
    state = Path(os.environ["ABBEY_STATE_DIR"])
    state.relative_to(root)
    if not (state / "seed-observed").is_file():
        raise SystemExit("CLI seed never executed")
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as client:
        client.settimeout(2)
        client.connect(os.environ["ABBEYD_SOCKET_PATH"])
        client.sendall(b"identity\n")
        identity = client.recv(128).decode().strip()
    record(event="test-daemon", identity=identity)
    # Deliberately let a stale but reachable decoy pass the fake Rust stage.
    # The outer regression must detect the shell's wrong artifact admission.
    print("running 2 tests")
    print("test result: ok. 2 passed; 0 failed; 0 ignored")
else:
    raise SystemExit("unexpected cargo command")
'''


ARTIFACT_STUB = r'''
import json
import os
from pathlib import Path
import signal
import socket
import sys
import time

root = Path(os.environ["ABBEY_PROOF_FIXTURE_ROOT"]).resolve()
events = Path(os.environ["ABBEY_PROOF_FIXTURE_EVENTS"])
name, identity, executable = sys.argv[1:4]
Path(executable).resolve().relative_to(root)
state = Path(os.environ["ABBEY_STATE_DIR"]).resolve()
state.relative_to(root)

def record(**event):
    with events.open("a") as stream:
        stream.write(json.dumps(event) + "\n")

record(event="artifact", name=name, identity=identity,
       executable=executable, pid=os.getpid())
if name == "abbey":
    if sys.argv[4:6] != ["memory", "put"]:
        raise SystemExit("unexpected CLI invocation")
    (state / "seed-observed").write_text(identity)
    raise SystemExit(0)
if name != "abbeyd":
    raise SystemExit("unknown artifact")

socket_path = Path(os.environ["ABBEYD_SOCKET_PATH"]).resolve()
socket_path.relative_to(state)
stopped = False

def stop(_signum, _frame):
    global stopped
    stopped = True

signal.signal(signal.SIGTERM, stop)
signal.signal(signal.SIGINT, stop)
deadline = time.monotonic() + 12
try:
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as listener:
        listener.bind(str(socket_path))
        listener.listen(2)
        listener.settimeout(0.1)
        while not stopped and time.monotonic() < deadline:
            try:
                client, _ = listener.accept()
            except socket.timeout:
                continue
            with client:
                client.settimeout(1)
                if client.recv(128) != b"identity\n":
                    raise SystemExit("unexpected fixture request")
                client.sendall(identity.encode() + b"\n")
finally:
    socket_path.unlink(missing_ok=True)
    record(event="daemon-stopped", identity=identity, pid=os.getpid())
'''


@unittest.skipUnless(sys.platform in ("darwin", "linux"), "Unix shell/socket proof")
class DaemonReadTarget(unittest.TestCase):
    def exercise(self, configured_target):
        with tempfile.TemporaryDirectory(prefix="abbey-proof fixture-", dir="/tmp") as temporary:
            root = Path(temporary).resolve()
            fixture_script = root / "desktop" / "scripts" / "prove-daemon-read.sh"
            fixture_script.parent.mkdir(parents=True)
            fixture_script.write_bytes(SCRIPT.read_bytes())
            (root / "Cargo.toml").write_text(
                '[package]\nname="fixture"\nversion="0.0.0"\nedition="2024"\n'
            )
            (root / "artifact_stub.py").write_text(ARTIFACT_STUB)
            (root / "fake_tools.py").write_text(FAKE_CARGO)
            events_path = root / "events.jsonl"
            fake_bin = root / "fake-bin"
            fake_bin.mkdir()
            for tool in ("cargo", "mktemp"):
                command = [sys.executable, "-I", str(root / "fake_tools.py"), tool]
                wrapper = fake_bin / tool
                wrapper.write_text("#!/bin/sh\nexec " + shlex.join(command) + ' "$@"\n')
                wrapper.chmod(0o700)

            # Stale canonical artifacts are executable and usable, reproducing
            # the dangerous successful-looking case, rather than mere ENOENT.
            for name in ("abbeyd", "abbey"):
                executable = root / "target" / "debug" / name
                executable.parent.mkdir(parents=True, exist_ok=True)
                command = [sys.executable, "-I", str(root / "artifact_stub.py"),
                           name, "stale", str(executable)]
                executable.write_text("#!/bin/sh\nexec " + shlex.join(command) + ' "$@"\n')
                executable.chmod(0o700)

            # Do not copy ambient environment: no credentials, provider config,
            # managed socket, Cargo binary, or runtime recipe is inherited.
            env = {"PATH": str(fake_bin) + ":/usr/bin:/bin",
                   "ABBEY_BACKEND": "abi", "CARGO_NET_OFFLINE": "true",
                   "ABBEY_PROOF_FIXTURE_ROOT": str(root),
                   "ABBEY_PROOF_FIXTURE_EVENTS": str(events_path)}
            if configured_target == "absolute":
                env["CARGO_TARGET_DIR"] = str(root / "selected build")
            elif configured_target == "relative":
                env["CARGO_TARGET_DIR"] = "intermediate/../selected build"
            elif configured_target == "env-host-target":
                env["CARGO_BUILD_TARGET"] = "x86_64-unknown-linux-gnu"
            elif configured_target == "config-host-target":
                config = root / ".cargo" / "config.toml"
                config.parent.mkdir()
                config.write_text('[build]\ntarget = "x86_64-unknown-linux-gnu"\n')
            elif configured_target is not None:
                self.fail("unknown target fixture")
            expected_target = (root / env.get("CARGO_TARGET_DIR", "target")).resolve()
            if configured_target in ("env-host-target", "config-host-target"):
                expected_target /= "x86_64-unknown-linux-gnu"

            process = subprocess.Popen(["/bin/sh", str(fixture_script)], cwd=root,
                                       env=env, stdout=subprocess.PIPE,
                                       stderr=subprocess.STDOUT, text=True,
                                       start_new_session=True)
            try:
                output, _ = process.communicate(timeout=15)
            finally:
                # Only this test's new process group can be signalled. Keep
                # cleanup bounded even if a future script loses its trap.
                try:
                    os.killpg(process.pid, signal.SIGTERM)
                except ProcessLookupError:
                    pass
                if process.poll() is None:
                    try:
                        process.wait(timeout=2)
                    except subprocess.TimeoutExpired:
                        os.killpg(process.pid, signal.SIGKILL)
                        process.wait(timeout=2)

            events = [json.loads(line) for line in events_path.read_text().splitlines()]
            self.assertEqual(process.returncode, 0, output[-4000:])
            self.assertIn("prove-daemon-read: OK", output)
            self.assertEqual([event["target"] for event in events
                              if event["event"] == "built"], [str(expected_target)])
            artifacts = [event for event in events if event["event"] == "artifact"]
            self.assertEqual([(event["name"], event["identity"]) for event in artifacts],
                             [("abbeyd", "active"), ("abbey", "active")],
                             "script launched stale canonical artifacts despite building another target")
            self.assertEqual([event["executable"] for event in artifacts],
                             [str(expected_target / "debug" / "abbeyd"),
                              str(expected_target / "debug" / "abbey")])
            self.assertEqual([event["identity"] for event in events
                              if event["event"] == "test-daemon"], ["active"])
            self.assertEqual([event["identity"] for event in events
                              if event["event"] == "daemon-stopped"], ["active"])
            self.assertFalse(list(root.glob("script-scratch-*")), "script scratch leaked")

    def test_default_target_builds_and_launches_fresh_owned_artifacts(self):
        self.exercise(None)

    def test_absolute_target_launches_selected_artifacts_not_stale_canonical(self):
        self.exercise("absolute")

    def test_relative_target_launches_selected_artifacts_not_stale_canonical(self):
        self.exercise("relative")

    def test_env_build_target_launches_fresh_artifacts_not_stale_host_profile(self):
        self.exercise("env-host-target")

    def test_config_build_target_launches_fresh_artifacts_not_stale_host_profile(self):
        self.exercise("config-host-target")


if __name__ == "__main__":
    unittest.main()
