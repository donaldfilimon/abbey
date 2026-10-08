#!/usr/bin/env python3
"""Actual all-peers-missing CLI/TUI notices with a private child PATH."""
import argparse
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile

sys.dont_write_bytecode = True
import smoke_tui_slash_pty as harness

COMMAND = "/subagents run --peers gemini synthetic-owned-missing-peer"
NOTICE = b"peer `gemini` not on PATH - skipping"
REFUSAL = b"no runnable subagents (all peers missing?)"


def check_private_path(owned):
    harness.require(owned.env["PATH"] == str(owned.root / "bin") and
                    not (owned.root / "bin" / "gemini").exists(),
                    "control: missing peer must use the private child PATH")


def control(binary, root):
    owned = harness.Fixture(root, backend="abi")
    check_private_path(owned)
    owned.set_control(marker="must-not-contact-any-agent")
    proc = subprocess.Popen([str(binary), COMMAND], cwd=root, env=owned.env,
                            stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                            stderr=subprocess.PIPE, start_new_session=True)
    try:
        output, errors = proc.communicate(timeout=harness.WAIT)
    except subprocess.TimeoutExpired:
        os.killpg(proc.pid, signal.SIGKILL)
        proc.wait(timeout=2)
        raise AssertionError("control: all-missing-peer CLI did not settle")
    harness.require(proc.returncode == 1 and NOTICE in errors and REFUSAL in errors,
                    f"control: unchanged CLI refusal/warning missing: exit={proc.returncode}, "
                    f"out={output[-400:]!r}, err={errors[-800:]!r}")
    harness.require(not owned.events(owned.nonce),
                    "control: all-missing-peer refusal contacted an agent")
    harness.require(not harness.exists(proc.pid, True),
                    "control: completed CLI retained an owned process group")


def retained(binary, root):
    owned = harness.Fixture(root, backend="abi")
    check_private_path(owned)
    session = harness.OwnedSession(binary, owned)
    try:
        # Startup is complete before this nonce; only the submitted work is
        # counted. No global environment or ambient peer binary is consulted.
        owned.set_control(marker="must-not-contact-any-agent")
        session.submit(COMMAND)
        session.wait_join(1)
        session.redraw()
        harness.require(REFUSAL in session.screen(),
                        "control: actual all-missing-peer refusal did not survive redraw")
        harness.require(NOTICE in session.screen(),
                        "expectation: missing-peer warning was not retained in the transcript")
        harness.require(not owned.events(owned.nonce),
                        "expectation: all-missing-peer refusal contacted an agent")
        session.quit_observed()
    finally:
        owned.cleanup(session)


def main():
    harness.OPTIONS = OPTIONS
    test = {"control": control, "tui": retained}[OPTIONS.mode]
    try:
        with tempfile.TemporaryDirectory(prefix="abbey-missing-peer-", dir="/tmp") as scratch:
            test(Path(OPTIONS.binary).resolve(), Path(scratch))
        proof = dict(status="passed", scenarios=[OPTIONS.mode], failures=[])
    except Exception as error:
        proof = dict(status="failed", scenarios=[], failures=[str(error)])
    print(json.dumps(proof, sort_keys=True))
    return 0 if proof["status"] == "passed" else 1


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("binary")
    parser.add_argument("mode", choices=("control", "tui"))
    parser.add_argument("--state-env", required=True)
    parser.add_argument("--config-env", required=True)
    OPTIONS = parser.parse_args()
    sys.exit(main())
