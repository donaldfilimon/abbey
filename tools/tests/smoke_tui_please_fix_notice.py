#!/usr/bin/env python3
"""Actual native Ctrl-P/slash context notices retained after terminal redraw."""
import argparse
import json
from pathlib import Path
import sys
import tempfile

sys.dont_write_bytecode = True
import smoke_tui_slash_pty as harness

SIGNAL = "error: synthetic-owned-please-fix-context"
NOTICE = b"please-fix capture"


def fixture(root):
    owned = harness.Fixture(root, backend="abi")
    capture = root / "captured-failure.txt"
    capture.write_text("owned-synthetic-command\n" + SIGNAL + "\n1\n")
    capture.chmod(0o600)
    # Only the child environment changes, never process-global test state.
    owned.env["CURSOR_AGENT_COMPLETED_PATH"] = str(capture)
    return owned


def check_call(owned, record):
    harness.require(record["backend"] == "abi" and record["argv"][0] == "complete"
                    and SIGNAL in " ".join(record["argv"]),
                    "control: actual please-fix did not forward the owned captured error")


def control(binary, root):
    owned = fixture(root)
    owned.set_control(marker="owned-please-fix-cli-answer")
    output, errors = owned.cli(binary, ["please-fix"])
    harness.require(NOTICE in errors and owned.marker.encode() in output,
                    "control: ordinary CLI please-fix warning or answer changed")
    calls = owned.events(owned.nonce)
    harness.require(len(calls) == 1, "control: ordinary please-fix did not execute once")
    check_call(owned, calls[0])


def retained(binary, root, key):
    owned = fixture(root)
    session = harness.OwnedSession(binary, owned)
    try:
        owned.set_control(marker="owned-please-fix-tui-answer")
        if key == "ctrl-p":
            session.send(b"\x10")
        else:
            session.submit("/please-fix ")
        record = owned.wait_call(session, "generation")
        check_call(owned, record)
        session.wait_join(0)
        session.redraw()
        harness.require(owned.marker.encode() in session.screen(),
                        "control: actual provider answer did not survive redraw")
        harness.require(NOTICE in session.screen(),
                        "expectation: please-fix capture warning was not retained in the transcript")
        owned.assert_closed(record, session)
        session.quit_observed()
    finally:
        owned.cleanup(session)


def main():
    harness.OPTIONS = OPTIONS
    case = {"control": control,
            "ctrl-p": lambda binary, root: retained(binary, root, "ctrl-p"),
            "slash": lambda binary, root: retained(binary, root, "slash")}[OPTIONS.mode]
    try:
        with tempfile.TemporaryDirectory(prefix="abbey-please-fix-notice-") as scratch:
            case(Path(OPTIONS.binary).resolve(), Path(scratch))
        proof = dict(status="passed", scenarios=[OPTIONS.mode], failures=[])
    except Exception as error:
        proof = dict(status="failed", scenarios=[], failures=[str(error)])
    print(json.dumps(proof, sort_keys=True))
    return 0 if proof["status"] == "passed" else 1


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("binary")
    parser.add_argument("mode", choices=("control", "ctrl-p", "slash"))
    parser.add_argument("--state-env", required=True)
    parser.add_argument("--config-env", required=True)
    OPTIONS = parser.parse_args()
    sys.exit(main())
