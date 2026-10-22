#!/usr/bin/env python3
"""A termination signal must leave the terminal restored, not broken.

The TUI owns the terminal while it runs: raw mode, the alternate screen,
and a hidden cursor. A signal that arrives from outside — `kill -INT`,
`kill -TERM`, a hangup — used to take the default disposition, so the
process died where it stood, the guard's `Drop` never ran, and the user
was left with echo off and the alternate screen still up (the shell needs
`reset`).

This harness drives the real binary in a real pty and asserts that the
terminal is put back: the alternate screen is left, the cursor is shown,
and the terminal's own attributes are the ones it had before.

Never run this against an installed binary: it needs the gate's debug
build.
"""
import json
import os
from pathlib import Path
import signal
import sys
import tempfile
import termios
import time

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parent))
from smoke_tui_pty import Session

# A ceiling, not a sleep: the wait ends as soon as the process is gone.
WAIT = 20.0


def executable(path, text):
    path.write_text(text)
    path.chmod(0o700)


def run(binary):
    proofs = []
    with tempfile.TemporaryDirectory(prefix="abbey-tui-signal-") as scratch:
        root = Path(scratch)
        executable(root / "agent", "#!/bin/sh\n"
                   'case "$1" in --version|version) printf "fixture-version\\n"; exit 0 ;; esac\n'
                   "printf 'fixture\\n'\n")
        for name, number in [("sigint", signal.SIGINT), ("sigterm", signal.SIGTERM)]:
            session = Session(binary, root)
            try:
                # The TUI is up and owns the terminal. The signal comes
                # from outside, the way a supervisor or a closing terminal
                # would send it.
                os.kill(session.proc.pid, number)
                deadline = time.monotonic() + WAIT
                while session.proc.poll() is None and time.monotonic() < deadline:
                    session.read()
                assert session.proc.poll() is not None, (
                    f"the TUI did not leave on {name}: the terminal would stay broken"
                )
                session.read(0.2)
                assert session.proc.poll() == 0, (
                    f"the TUI left through an error on {name}: {session.proc.returncode}"
                )
                # The terminal is the user's again: the alternate screen is
                # left, the cursor is shown, and the attributes are back.
                assert termios.tcgetattr(session.slave) == session.before, (
                    f"the terminal attributes were not restored after {name}"
                )
                for mode in [b"\x1b[?1049l", b"\x1b[?25h"]:
                    assert mode in session.output, (
                        f"the terminal reset after {name} is missing {mode!r}"
                    )
                proofs.append(f"{name}-restores-terminal")
            finally:
                session.abort()
    print(json.dumps({"status": "passed", "scenarios": proofs}, sort_keys=True))


if __name__ == "__main__":
    run(Path(sys.argv[1]).resolve())
