#!/usr/bin/env python3
"""Actual /doctor PTY metadata owner; private selected ABI and no providers."""
import argparse
import json
from pathlib import Path
import sys
import tempfile
import time

sys.dont_write_bytecode = True
import smoke_tui_slash_pty as harness

VERSION_BLOCK = '''if args in (["--version"], ["version"]):
    print("owned-fixture-version")
    sys.exit(0)
'''
OWNED_VERSION_BLOCK = r'''if args in (["--version"], ["version"]):
    if control.get("action") != "metadata":
        print("owned-fixture-version")
        sys.exit(0)
    leaf = None
    if control.get("hold"):
        leaf = subprocess.Popen([sys.executable, sys.argv[0], "--fixture-leaf"])
    def close_metadata_leaf():
        if leaf is not None:
            if leaf.poll() is None:
                try: leaf.terminate()
                except ProcessLookupError: pass
            try: leaf.wait(timeout=1)
            except subprocess.TimeoutExpired:
                leaf.kill(); leaf.wait(timeout=1)
    def cancel_metadata(*_):
        write_event("terminated", action="metadata")
        close_metadata_leaf()
        sys.exit(130)
    signal.signal(signal.SIGTERM, cancel_metadata)
    write_event("call", action="metadata", leaf_pid=leaf.pid if leaf else None)
    if control.get("hold"):
        deadline = time.monotonic() + float(os.environ["ABBEY_FIXTURE_HOLD_SECONDS"])
        while not (root / "release").exists() and time.monotonic() < deadline:
            time.sleep(.01)
        close_metadata_leaf()
    print(control["marker"], flush=True)
    sys.exit(0)
'''


class MetadataFixture(harness.Fixture):
    def __init__(self, root):
        super().__init__(root, backend="abi")
        harness.require(harness.STUB.count(VERSION_BLOCK) == 1,
                        "fixture compatibility: original version response block changed")
        stub = "#!" + sys.executable + "\n" + harness.STUB.replace(
            VERSION_BLOCK, OWNED_VERSION_BLOCK, 1)
        binary = root / "bin" / "abi"
        binary.write_text(stub)
        binary.chmod(0o700)

    def assert_exact_probe(self, record):
        harness.require(record["backend"] == "abi" and
                        record["argv"] == ["--version"] and
                        record["path"] == str((self.root / "bin" / "abi").resolve()),
                        "control: doctor did not execute the exact selected ABI --version")

    def wait_leaf(self, session, record):
        deadline = time.monotonic() + harness.WAIT
        while time.monotonic() < deadline:
            rows = [r for r in self.events(self.nonce, "leaf")
                    if r["pid"] == record.get("leaf_pid")]
            if rows:
                harness.require(rows[0]["pgid"] == record["pgid"],
                                "control: metadata leaf escaped its own executor group")
                harness.require(harness.exists(record["pid"]) and
                                harness.exists(record["leaf_pid"]),
                                "control: metadata owner/leaf were not held at readiness")
                return
            session.read(.005)
        raise AssertionError("control: actual metadata leaf readiness was not observed")


def control(binary, root):
    fixture = MetadataFixture(root)
    # This proves the unchanged ordinary CLI protocol separately from the TUI.
    fixture.set_control("metadata", marker="owned-doctor-version-control")
    output, _ = fixture.cli(binary, ["/doctor"])
    harness.require(fixture.marker.encode() in output,
                    "control: ordinary doctor omitted the actual version response")
    rows = fixture.events(fixture.nonce)
    metadata = [r for r in rows if r["action"] == "metadata"]
    harness.require(len(metadata) == 1,
                    "control: ordinary doctor did not use one actual metadata probe")
    fixture.assert_exact_probe(metadata[0])
    session = harness.OwnedSession(binary, fixture)
    try:
        fixture.set_control("metadata", marker="owned-doctor-tui-control")
        session.submit("/doctor ")
        record = fixture.wait_call(session, "metadata")
        fixture.assert_exact_probe(record)
        session.wait_join(0)
        fixture.assert_closed(record, session, include_parent=record["ppid"] != session.proc.pid)
        session.quit_observed()
    finally:
        fixture.cleanup(session)


def cancellation(binary, root, quit_run=False):
    fixture = MetadataFixture(root)
    # Startup probes remain immediate. Hold only the post-startup /doctor nonce.
    session = harness.OwnedSession(binary, fixture)
    try:
        fixture.set_control("metadata", hold=True, marker="must-not-publish-held-version")
        session.submit("/doctor ")
        record = fixture.wait_call(session, "metadata")
        fixture.assert_exact_probe(record)
        fixture.wait_leaf(session, record)
        harness.require(harness.exists(record["ppid"]),
                        "control: actual metadata supervisor owner was absent")
        if quit_run:
            session.quit_observed()
        else:
            session.submit("metadata-queued-never-contact")
            session.wait_output(b"queued (1)", timeout=harness.WAIT)
            session.send(b"\x1b")
            session.wait_join(130)
            session.wait_output(b"discarded 1 queued prompt", timeout=harness.WAIT)
        # These assertions precede release and all failure cleanup. The broken
        # outer Abbey group can join while its separate version group survives.
        fixture.assert_closed(record, session, include_parent=record["ppid"] != session.proc.pid)
        harness.require(not any("metadata-queued-never-contact" in " ".join(r["argv"])
                                for r in fixture.events()),
                        "expectation: metadata cancellation replayed queued generation")
        if not quit_run:
            session.quit_observed()
    finally:
        fixture.cleanup(session)


def main():
    harness.OPTIONS = OPTIONS
    test = {"control": control,
            "cancel": cancellation,
            "quit": lambda binary, root: cancellation(binary, root, True)}[OPTIONS.mode]
    scenario = "doctor-metadata-" + OPTIONS.mode
    try:
        with tempfile.TemporaryDirectory(prefix="abbey-metadata-pty-") as scratch:
            test(Path(OPTIONS.binary).resolve(), Path(scratch))
        proof = dict(status="passed", mode=OPTIONS.mode,
                     scenarios=[dict(scenario=scenario, status="passed")], failures=[])
    except Exception as error:
        proof = dict(status="failed", mode=OPTIONS.mode, scenarios=[],
                     failures=[dict(scenario=scenario, status="failed", error=str(error))])
    print(json.dumps(proof, sort_keys=True))
    return 0 if proof["status"] == "passed" else 1


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("binary")
    parser.add_argument("mode", choices=("control", "cancel", "quit"))
    parser.add_argument("--state-env", required=True)
    parser.add_argument("--config-env", required=True)
    OPTIONS = parser.parse_args()
    sys.exit(main())
