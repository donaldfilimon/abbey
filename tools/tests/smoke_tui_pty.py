#!/usr/bin/env python3
"""Real PTY acceptance with isolated state and deterministic local executors."""
import fcntl
import json
import os
from pathlib import Path
import select
import re
import unicodedata
import signal
import struct
import subprocess
import sys
import tempfile
import termios
import time


class Session:
    def __init__(self, binary, root, editor=None):
        self.root = root
        self.master, self.slave = os.openpty()
        self.resize(30, 100)
        self.before = termios.tcgetattr(self.slave)
        env = {k: v for k, v in os.environ.items()
               if not k.startswith(("ABBEY", "ABBEYD", "CURSOR", "ANTHROPIC", "OPENAI", "XAI", "GROK"))}
        env.update(HOME=str(root / "home"), XDG_CONFIG_HOME=str(root / "config"),
                   XDG_STATE_HOME=str(root / "xdg-state"), TERM="xterm-256color",
                   ABBEY_STATE_DIR=str(root / "state"), ABBEY_CONFIG=str(root / "config.toml"),
                   ABBEY_BACKEND="abi", ABBEY_ABI_BIN=str(root / "agent"),
                   ABBEY_PER_CWD="0", VISUAL=str(editor or root / "missing-editor"))
        for name in ["home", "config", "xdg-state", "state"]:
            (root / name).mkdir(exist_ok=True)
        (root / "config.toml").write_text('backend = "abi"\n')
        self.output = bytearray()
        self.query_offset = 0
        self.cursor = (1, 1)

        self.proc = subprocess.Popen([str(binary), "tui"], cwd=root, env=env,
                                     stdin=self.slave, stdout=self.slave, stderr=self.slave,
                                     start_new_session=True)
        try:
            self.wait_output(b"Enter send")
        except BaseException:
            self.abort()
            raise

    def resize(self, rows, cols):
        self.rows, self.cols = rows, cols
        fcntl.ioctl(self.slave, termios.TIOCSWINSZ, struct.pack("HHHH", rows, cols, 0, 0))
        if hasattr(self, "proc"):
            os.kill(self.proc.pid, signal.SIGWINCH)

    def read(self, wait=0.05):
        if select.select([self.master], [], [], wait)[0]:
            chunk = os.read(self.master, 65536)
            self.output.extend(chunk)
            assert len(self.output) <= 8 * 1024 * 1024, "PTY output exceeded harness ceiling"
            # A PTY supplies bytes, not a terminal emulator. Answer the cursor
            # position query crossterm uses during resize/clear.
            while True:
                query = self.output.find(b"\x1b[6n", self.query_offset)
                if query < 0: break
                self.screen()
                row, col = self.cursor
                self.send(f"\x1b[{row};{col}R".encode())
                self.query_offset = query + 4

    def screen(self):
        rows, cols = self.rows, self.cols
        grid = [[" " for _ in range(cols)] for _ in range(rows)]
        row = col = 0
        for token in re.findall(r"\x1b\[[0-?]*[ -/]*[@-~]|\x1b.|.", self.output.decode("utf-8", "replace"), re.S):
            if token.startswith("\x1b["):
                body, op = token[2:-1], token[-1]
                if body.startswith("?"):
                    if body == "?1049" and op == "h":
                        grid = [[" " for _ in range(cols)] for _ in range(rows)]
                        row = col = 0
                    continue
                values = [int(v) if v else 0 for v in body.split(";")] if body else [0]
                n = values[0] or 1
                if op in "Hf":
                    row = max(0, n - 1)
                    col = max(0, (values[1] if len(values) > 1 else 1) - 1)
                elif op == "G": col = n - 1
                elif op == "d": row = n - 1
                elif op == "A": row = max(0, row - n)
                elif op == "B": row += n
                elif op == "C": col += n
                elif op == "D": col = max(0, col - n)
                elif op == "J" and values[0] in (2, 3):
                    grid = [[" " for _ in range(cols)] for _ in range(rows)]
                elif op == "K" and row < rows:
                    lo, hi = (0, cols) if values[0] == 2 else ((0, min(cols, col + 1)) if values[0] == 1 else (min(cols, col), cols))
                    grid[row][lo:hi] = [" "] * (hi - lo)
                continue
            if token.startswith("\x1b"): continue
            if token == "\r": col = 0; continue
            if token == "\n": row += 1; continue
            if ord(token) < 32 or unicodedata.combining(token): continue
            if row < rows and col < cols:
                grid[row][col] = token
            col += 2 if unicodedata.east_asian_width(token) in ("W", "F") else 1
        self.cursor = (min(rows, row + 1), min(cols, col + 1))
        return "\n".join("".join(line) for line in grid).encode()

    def wait_output(self, needle, after=0, timeout=10):
        deadline = time.monotonic() + timeout
        while needle not in self.screen() and time.monotonic() < deadline:
            self.read()
            if self.proc.poll() is not None:
                break
        assert needle in self.screen(), f"missing PTY marker {needle!r}; exit={self.proc.poll()}; screen={self.screen()!r}; tail={bytes(self.output[-800:])!r}"

    def send(self, text):
        os.write(self.master, text)

    def paste(self, text):
        self.send(b"\x1b[200~" + text.encode() + b"\x1b[201~")

    def wait_file(self, name, timeout=10):
        deadline = time.monotonic() + timeout
        while not (self.root / name).exists() and time.monotonic() < deadline:
            self.read()
        assert (self.root / name).exists(), f"fixture did not reach {name}"

    def finish(self):
        self.send(b"\x03")
        self.wait_output(b"Ctrl-C again to quit")
        self.send(b"\x03")
        deadline = time.monotonic() + 10
        while self.proc.poll() is None and time.monotonic() < deadline:
            self.read()
        assert self.proc.poll() == 0, "TUI failed to quit within teardown bound"
        self.read(0)
        assert termios.tcgetattr(self.slave) == self.before, "terminal attributes were not restored"
        for mode in [b"\x1b[?1049l", b"\x1b[?1000l", b"\x1b[?2004l", b"\x1b[?25h"]:
            assert mode in self.output, f"terminal reset missing {mode!r}"
        os.close(self.master)
        os.close(self.slave)

    def abort(self):
        if self.proc.poll() is None:
            # Give Abbey the chance to reap its separately supervised executor
            # group before using the harness's last-resort process kill.
            self.send(b"\x03\x03")
            deadline = time.monotonic() + 5
            while self.proc.poll() is None and time.monotonic() < deadline:
                self.read()
            if self.proc.poll() is None:
                os.killpg(self.proc.pid, signal.SIGKILL)
                self.proc.wait()
            marker = self.root / "agent.pid"
            if marker.exists():
                try:
                    pid = int(marker.read_text())
                    command = subprocess.run(["ps", "-p", str(pid), "-o", "command="],
                                             capture_output=True, text=True, check=False).stdout
                    if str(self.root / "agent") in command:
                        os.killpg(pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass


def executable(path, text):
    path.write_text(text)
    path.chmod(0o700)


def run(binary):
    proofs = []
    with tempfile.TemporaryDirectory(prefix="abbey-tui-pty-") as scratch:
        root = Path(scratch)
        executable(root / "agent", "#!/bin/sh\n"
                   'case "$1" in --version|version) printf "fixture-version\\n"; exit 0 ;; esac\n' 
                   "printf '%s' \"$$\" > agent.pid\n"
                   "printf '%s\\n' \"$*\" >> calls.log\n"
                   "case \"$*\" in\n"
                   " *cancel-fixture*|*quit-fixture*) sleep 30 & child=$!; printf '%s' \"$child\" > child.pid; wait ;;\n"
                   " *) printf 'fixture-stream-first'; touch first.chunk; "
                   "while [ ! -f continue.stream ]; do sleep 0.02; done; "
                   "printf '\\nfixture-stream-last\\n' ;;\n"
                   "esac\n")
        executable(root / "editor", "#!/bin/sh\nprintf 'editor-draft  \\n' > \"$1\"\n")
        # Multiline bracketed paste must stay a draft until explicit submission;
        # first streaming bytes must render while the fixture is still alive.
        session = Session(binary, root, root / "editor")
        try:
            session.paste("paste-one\npaste-two 🌍")
            session.wait_output("paste-two 🌍".encode())
            assert not (root / "calls.log").exists(), "paste submitted without Enter"
            session.resize(16, 42)
            session.send(b"\r")
            session.wait_output(b"fixture-stream-first")
            assert session.proc.poll() is None
            (root / "continue.stream").touch()
            session.wait_output(b"fixture-stream-last")
            session.resize(30, 100)
            session.wait_output(b"exit 0")
            session.send(b"\x07")  # Ctrl-G, successful editor return
            session.wait_output(b"editor-draft")
            session.finish()
            proofs += ["multiline-paste", "incremental-stream", "resize", "editor-return", "terminal-restoration"]
        finally:
            session.abort()
        # Missing editor returns to the TUI with the original draft intact.
        session = Session(binary, root)
        try:
            session.paste("keep-editor-draft")
            session.send(b"\x07")
            session.wait_output(b"suspended command:")
            session.wait_output(b"keep-editor-draft")
            session.finish()
            proofs.append("failed-editor-recovery")
        finally:
            session.abort()
        # Interrupt a live group after queueing work; no queued invocation follows.
        (root / "child.pid").unlink(missing_ok=True)
        session = Session(binary, root)
        try:
            session.paste("cancel-fixture")
            session.send(b"\r")
            session.wait_file("child.pid")
            pid = int((root / "child.pid").read_text())
            session.paste("queued-must-not-run")
            session.send(b"\r")
            session.wait_output(b"queued (1)")
            session.send(b"\x1b")
            session.wait_output(b"discarded 1")
            assert "queued-must-not-run" not in (root / "calls.log").read_text()
            try:
                os.kill(pid, 0)
                raise AssertionError("interrupted descendant survived")
            except ProcessLookupError:
                pass
            session.finish()
            proofs += ["interrupt-discards-queue", "descendant-teardown"]
        finally:
            session.abort()
        # Quit while running must wait for supervised teardown.
        (root / "child.pid").unlink(missing_ok=True)
        session = Session(binary, root)
        try:
            session.paste("quit-fixture")
            session.send(b"\r")
            session.wait_file("child.pid")
            pid = int((root / "child.pid").read_text())
            session.finish()
            try:
                os.kill(pid, 0)
                raise AssertionError("quit left a descendant alive")
            except ProcessLookupError:
                pass
            proofs.append("quit-teardown")
        finally:
            session.abort()
    print(json.dumps({"status": "passed", "scenarios": proofs}, sort_keys=True))


if __name__ == "__main__":
    run(Path(sys.argv[1]).resolve())
