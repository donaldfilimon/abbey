#!/usr/bin/env python3
"""Actual /doctor PTY metadata owner; private selected ABI and no providers."""
import argparse
import json
import hashlib
import os
import subprocess
from pathlib import Path
import sys
import tempfile
import time

sys.dont_write_bytecode = True
import smoke_tui_slash_pty as harness

class MetadataFixture(harness.Fixture):
    def __init__(self, root):
        self.generation_python = str(Path(sys.executable).absolute())
        self.generation_script = str((root / "generation" / "abi").resolve())
        super().__init__(root, backend="abi")
        generation = Path(self.generation_script)
        generation.parent.mkdir()
        generation.write_text(harness.STUB)
        source = Path(__file__).with_name("metadata_probe_fixture.c")
        compiler = os.environ.get("CC", "/usr/bin/cc")
        harness.require(Path(compiler).is_absolute(), "native fixture CC must be an absolute executable path")
        command = [compiler, "-std=c99", "-D_XOPEN_SOURCE=700", "-Wall", "-Wextra",
                   str(source), "-o", str(root / "bin" / "abi")]
        compiled = subprocess.run(command, capture_output=True, text=True, timeout=30)
        harness.require(compiled.returncode == 0,
                        "native fixture compiler failed: " + compiled.stderr[-8192:])
        version = subprocess.run([compiler, "--version"], capture_output=True, text=True, check=True, timeout=10)
        (root / "native-compiler-proof.json").write_text(json.dumps(dict(
            command=command, compiler_version=version.stdout[:4096],
            source_sha256=hashlib.sha256(source.read_bytes()).hexdigest())))

    def set_control(self, action="generation", hold=False, marker=None):
        super().set_control(action, hold, marker)
        fields = [self.nonce, str(int(action == "metadata")), str(int(hold)),
                  self.marker, self.generation_python, self.generation_script]
        harness.require(all("\n" not in v and "\r" not in v for v in fields),
                        "native fixture sidecar fields must be single lines")
        harness.require(len(self.marker) <= 255 and self.marker and
                        all(c in "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-_"
                            for c in self.marker), "native fixture marker outside bounded grammar")
        harness.require(all(len(v.encode()) < 4095 for v in fields), "native sidecar field too long")
        pending = self.root / "native-control.pending"
        pending.write_text("\n".join(fields) + "\n")
        pending.replace(self.root / "native-control")

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


def ordinary_control_probe(binary, fixture):
    # Cold interpreter/loader startup can consume a legitimate 500 ms probe
    # deadline before the owned stub records any event. These are explicit
    # read-only setup attempts, each with a new nonce. Never retry an observed
    # wrong response, and never count an unanswered attempt as positive proof.
    diagnostics = []
    for attempt in range(3):
        fixture.set_control("metadata", marker="owned-doctor-version-control")
        output, stderr = fixture.cli(binary, ["/doctor"])
        rows = fixture.events(fixture.nonce)
        if fixture.marker.encode() in output:
            fixture.control_readiness_attempts = attempt + 1
            return rows
        diagnostics.append(dict(attempt=attempt + 1, stdout=repr(output[-4096:]),
                                stderr=repr(stderr[-4096:]), owned_events=rows))
        harness.require(not rows,
                        "control: observed metadata response omitted its actual version; "
                        f"diagnostics={json.dumps(diagnostics, sort_keys=True)[:16384]}")
    raise AssertionError("control: ordinary doctor omitted the actual version response "
                         "after three explicit reads; "
                         f"diagnostics={json.dumps(diagnostics, sort_keys=True)[:32768]}")


def control(binary, root):
    fixture = MetadataFixture(root)
    # This proves the unchanged ordinary CLI protocol separately from the TUI.
    rows = ordinary_control_probe(binary, fixture)
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
    return dict(ordinary_readiness_attempts=fixture.control_readiness_attempts)


def cancellation(binary, root, quit_run=False):
    fixture = MetadataFixture(root)
    # Startup probes remain immediate. Hold only the post-startup /doctor nonce.
    session = harness.OwnedSession(binary, fixture)
    try:
        fixture.set_control("metadata", hold=True, marker="must-not-publish-held-version")
        if quit_run:
            session.submit("/doctor ")
        else:
            # Queue both Enter inputs before spending the real 500 ms metadata
            # deadline observing process readiness. Keep the normal bracketed
            # paste grammar; one write preserves the two submissions' order.
            session.send(b"\x1b[200~/doctor \x1b[201~\r"
                         b"\x1b[200~metadata-queued-never-contact\x1b[201~\r")
        record = fixture.wait_call(session, "metadata")
        fixture.assert_exact_probe(record)
        fixture.wait_leaf(session, record)
        harness.require(harness.exists(record["ppid"]),
                        "control: actual metadata supervisor owner was absent")
        if quit_run:
            session.quit_observed()
        else:
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
            setup = test(Path(OPTIONS.binary).resolve(), Path(scratch))
        proof = dict(status="passed", mode=OPTIONS.mode,
                     scenarios=[dict(scenario=scenario, status="passed")], failures=[])
        if setup is not None:
            proof["setup"] = setup
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
