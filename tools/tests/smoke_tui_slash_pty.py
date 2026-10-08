#!/usr/bin/env python3
"""Hermetic A3/A4/A5 PTY regressions; no live executor or inherited state.

Uses the existing terminal emulator, but constructs a strict owned environment.
Requires the gate's debug CARGO_BIN_EXE_abbey; the existing debug-only HOME
resolution restriction is essential. Never run this against an installed binary.
"""
import argparse
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import termios
import time
import uuid

sys.dont_write_bytecode = True
from smoke_tui_pty import Session

WAIT = 4.0
# Longer than every observed cancellation assertion; release is cleanup only.
HOLD = 20.0
STUB = r'''
import json, os, signal, subprocess, sys, time, uuid
from pathlib import Path
root = Path(os.environ["ABBEY_FIXTURE_ROOT"])
args = sys.argv[1:]
name = Path(sys.argv[0]).name

def write_event(kind, **extra):
    event = dict(kind=kind, nonce=control["nonce"], backend=name,
                 pid=os.getpid(), pgid=os.getpgrp(), ppid=os.getppid(),
                 path=str(Path(sys.argv[0]).resolve()), argv=args)
    try: event["parent_pgid"] = os.getpgid(os.getppid())
    except ProcessLookupError: event["parent_pgid"] = None
    event.update(extra)
    dest = root / "events" / (uuid.uuid4().hex + ".json")
    pending = dest.with_suffix(".pending")
    pending.write_text(json.dumps(event))
    pending.replace(dest)

control = json.loads((root / "control.json").read_text())
if args == ["--fixture-leaf"]:
    write_event("leaf")
    signal.signal(signal.SIGTERM, lambda *_: sys.exit(130))
    deadline = time.monotonic() + float(os.environ["ABBEY_FIXTURE_HOLD_SECONDS"])
    while not (root / "release").exists() and time.monotonic() < deadline:
        time.sleep(.01)
    sys.exit(0)
# Git is an owned, read-only response fixture. No actual repository mutation.
if name == "git":
    if "rev-parse" in args:
        print("true" if "--is-inside-work-tree" in args else "fixture-main")
    elif "ls-files" in args: print("fixture.txt")
    elif "diff" in args:
        print(" fixture.txt | 1 +" if "--stat" in args else
              "diff --git a/fixture.txt b/fixture.txt\n+synthetic-owned-change")
    else: sys.exit(1)
    sys.exit(0)
if args in (["--version"], ["version"]):
    print("owned-fixture-version")
    sys.exit(0)
if args and args[0] == "create-chat":
    print("owned-created-cursor-chat")
    sys.exit(0)
if name == "fm" and args == ["models"]:
    print("✓ system")
    sys.exit(0)
if args and args[0] in ("models", "status"):
    action = args[0]
elif name == "fm" and args and args[0] == "respond":
    action = "teacher"
elif name == "abi" and args and args[0] == "complete":
    action = "generation"
elif name == "cursor-agent":
    action = "generation"
else:
    sys.stderr.write("unsupported owned fixture protocol: " + repr(args))
    sys.exit(2)
# A held parent owns and reaps a same-group leaf. This makes group disappearance
# evidence meaningful without relying on the host's orphan-zombie reaper.
leaf = None
hold = bool(control.get("hold")) and action == control.get("action")
if hold:
    leaf = subprocess.Popen([sys.executable, sys.argv[0], "--fixture-leaf"])
write_event("call", action=action, leaf_pid=leaf.pid if leaf else None)

def close_leaf():
    if leaf is not None:
        if leaf.poll() is None:
            try: leaf.terminate()
            except ProcessLookupError: pass
        try: leaf.wait(timeout=1)
        except subprocess.TimeoutExpired:
            leaf.kill(); leaf.wait(timeout=1)

def cancelled(*_):
    write_event("terminated", action=action)
    close_leaf()
    sys.exit(130)
signal.signal(signal.SIGTERM, cancelled)
if hold:
    deadline = time.monotonic() + float(os.environ["ABBEY_FIXTURE_HOLD_SECONDS"])
    while not (root / "release").exists() and time.monotonic() < deadline:
        time.sleep(.01)
    close_leaf()
marker = control["marker"]
if name == "cursor-agent" and "stream-json" in args:
    print(json.dumps({"type":"assistant", "message":{"content":[
        {"type":"text", "text":marker}]}}), flush=True)
else:
    print(marker, flush=True)
'''


def require(condition, message):
    if not condition:
        raise AssertionError(message)


def exists(pid, group=False):
    if pid is None:
        return False
    try:
        (os.killpg if group else os.kill)(pid, 0)
        return True
    except ProcessLookupError:
        return False


class Fixture:
    def __init__(self, root, backend="cursor", teacher=False):
        self.root, self.backend = root, backend
        for name in ("home", "config", "xdg-state", "state", "bin", "events"):
            (root / name).mkdir(parents=True, exist_ok=True)
        self.nonce = uuid.uuid4().hex
        self.marker = "owned-answer-" + self.nonce[:8]
        self.set_control()
        stub = "#!" + sys.executable + "\n" + STUB
        names = ["cursor-agent", "abi", "git"] + (["fm"] if teacher else [])
        for name in names:
            path = root / "bin" / name
            path.write_text(stub)
            path.chmod(0o700)
        (root / "fixture.txt").write_text("synthetic-owned-change\n")
        (root / "config.toml").write_text(
            f'backend = "{backend}"\nmemory_backend = "sqlite"\n')
        # Both edition roots are owned, never inherited. The integration
        # wrapper identifies the active names, so personal builds stay isolated.
        self.env = {
            "HOME": str(root / "home"), "XDG_CONFIG_HOME": str(root / "config"),
            "XDG_STATE_HOME": str(root / "xdg-state"), "TERM": "xterm-256color",
            "PATH": str(root / "bin"), "ABBEY_FIXTURE_ROOT": str(root),
            "ABBEY_FIXTURE_HOLD_SECONDS": str(HOLD),
            "ABBEY_BACKEND": backend, "ABBEY_ABI_BIN": str(root / "bin" / "abi"),
            "ABBEY_TEST_HOME_AGENTS_ONLY": "1", "ABBEY_PER_CWD": "0",
            "ABBEY_AUTO_REVIEW": "0", "ABBEY_TRUST": "0", "ABBEY_FORCE": "0",
            "CURSOR_AGENT_CHAT_ID": "ambient-owned-cursor-chat",
            "VISUAL": str(root / "missing-editor"),
        }
        self.env[OPTIONS.state_env] = str(root / "state")
        self.env[OPTIONS.config_env] = str(root / "config.toml")
        (root / "state" / "chat-id").write_text("selected-owned-abi-chat\n")

    def set_control(self, action="generation", hold=False, marker=None):
        self.nonce = uuid.uuid4().hex
        self.marker = marker or "owned-answer-" + self.nonce[:8]
        pending = self.root / "control.pending"
        pending.write_text(json.dumps(dict(nonce=self.nonce, marker=self.marker,
                                           action=action, hold=hold)))
        pending.replace(self.root / "control.json")

    def events(self, nonce=None, kind="call"):
        rows = [json.loads(p.read_text()) for p in (self.root / "events").glob("*.json")]
        return [r for r in rows if r["kind"] == kind and
                (nonce is None or r["nonce"] == nonce)]

    def wait_call(self, session, action):
        end = time.monotonic() + WAIT
        while time.monotonic() < end:
            calls = [r for r in self.events(self.nonce) if r["action"] == action]
            if calls:
                return calls[-1]
            session.read()
        raise AssertionError(f"control: fixture did not enter actual {action} argv")

    def release(self):
        # This must be called only AFTER desired cancellation assertions.
        (self.root / "release").touch()

    def assert_closed(self, record, session, include_parent=False):
        # Exit 130 is drawn only after App::pump has joined the worker. Check
        # exact owned pids plus their isolated groups before any release/kill.
        end = time.monotonic() + 1.0
        pids = [record["pid"], record.get("leaf_pid")]
        if include_parent:
            pids.append(record["ppid"])
        groups = [record["pgid"]]
        if include_parent:
            groups.append(record["parent_pgid"])
        groups = {g for g in groups if g is not None and g != session.proc.pid}
        while time.monotonic() < end and (any(exists(p) for p in pids) or
                                           any(exists(g, True) for g in groups)):
            if session.proc.poll() is None:
                session.read(.01)
            else:
                time.sleep(.01)
        require(not any(exists(p) for p in pids),
                f"expectation: owned processes survived observed worker join: {pids}")
        require(not any(exists(g, True) for g in groups),
                f"expectation: owned process group survived observed join: {groups}")

    def cleanup(self, session):
        self.release()
        session.cleanup()
        # All stub calls are from this root/nonce, with finite release polling.
        # Do not signal arbitrary process trees or parent pids read from ps.
        end = time.monotonic() + 3
        rows = self.events()
        while time.monotonic() < end and any(exists(r["pid"]) for r in rows):
            time.sleep(.02)
        require(not any(exists(r["pid"]) for r in rows),
                "cleanup: an owned fixture did not settle after explicit release")

    def cli(self, binary, args):
        proc = subprocess.Popen([str(binary), *args], cwd=self.root, env=self.env,
                                stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                                stderr=subprocess.PIPE, start_new_session=True)
        try:
            out, err = proc.communicate(timeout=WAIT)
        except subprocess.TimeoutExpired:
            self.release()
            try:
                out, err = proc.communicate(timeout=2)
            except subprocess.TimeoutExpired:
                os.killpg(proc.pid, signal.SIGKILL)
                proc.wait(timeout=2)
            raise AssertionError("control: owned direct CLI did not settle")
        require(proc.returncode == 0,
                f"control: CLI exit={proc.returncode}, stderr={err[-800:]!r}")
        return out, err


class OwnedSession(Session):
    def __init__(self, binary, fixture):
        self.root = fixture.root
        self.master, self.slave = os.openpty()
        self.resize(30, 110)
        self.before = termios.tcgetattr(self.slave)
        self.output, self.query_offset, self.cursor = bytearray(), 0, (1, 1)
        self.closed = False
        self.proc = subprocess.Popen([str(binary), "tui"], cwd=self.root,
                                     env=fixture.env, stdin=self.slave,
                                     stdout=self.slave, stderr=self.slave,
                                     start_new_session=True)
        try:
            self.wait_output(b"Enter send", timeout=WAIT)
        except BaseException:
            fixture.release()
            self.cleanup()
            raise

    def submit(self, text):
        self.paste(text)
        self.send(b"\r")

    def wait_join(self, code):
        # Use rendered/current screen, never historical raw bytes. The previous
        # turn's exit marker can otherwise let a held turn appear complete.
        self.wait_output(f"done · exit {code}".encode(), timeout=WAIT)

    def redraw(self):
        self.resize(28, 108)
        end = time.monotonic() + .15
        while time.monotonic() < end:
            self.read(.02)
        self.resize(30, 110)
        end = time.monotonic() + .15
        while time.monotonic() < end:
            self.read(.02)

    def quit_observed(self):
        self.send(b"\x03")
        self.wait_output(b"Ctrl-C again to quit", timeout=WAIT)
        self.send(b"\x03")
        end = time.monotonic() + WAIT
        while self.proc.poll() is None and time.monotonic() < end:
            self.read(.02)
        require(self.proc.poll() == 0,
                "expectation: TUI quit did not observe bounded worker teardown")
        self.read(0)
        require(termios.tcgetattr(self.slave) == self.before,
                "expectation: terminal attributes not restored after actual quit")
        for mode in (b"\x1b[?1049l", b"\x1b[?1000l", b"\x1b[?2004l", b"\x1b[?25h"):
            require(mode in self.output, f"expectation: missing terminal reset {mode!r}")

    def cleanup(self):
        if self.closed:
            return
        try:
            if self.proc.poll() is None:
                # Caller's owned release is already published. Allow current
                # broken code to return before the last-resort TUI-only signal.
                self.send(b"\x03\x03")
                end = time.monotonic() + 2
                while self.proc.poll() is None and time.monotonic() < end:
                    self.read(.02)
                if self.proc.poll() is None:
                    os.killpg(self.proc.pid, signal.SIGKILL)
                self.proc.wait(timeout=2)
        finally:
            os.close(self.master)
            os.close(self.slave)
            self.closed = True


def ordinary_control(session, fixture):
    fixture.set_control(marker="ordinary-canonical-stream-control")
    session.submit("ordinary-owned-prompt")
    call = fixture.wait_call(session, "generation")
    session.wait_join(0)
    session.redraw()
    require(fixture.marker.encode() in session.screen(),
            "control: canonical streaming reply did not survive redraw")
    fixture.assert_closed(call, session)


def output_case(binary, root, name):
    f = Fixture(root)
    action = "generation" if name == "commit" else name
    f.set_control(action, marker=f"retained-{name}-owned-marker")
    # Direct headless/passthrough contract is a separate positive control.
    out, _ = f.cli(binary, [f"/{name}"])
    require(f.marker.encode() in out, f"control: direct /{name} lost its output")
    if name == "commit":
        control_calls = f.events(f.nonce)
        require(len(control_calls) == 1, "control: direct commit did not capture exactly once")
        args = control_calls[0]["argv"]
        require("--print" in args and "--resume" in args and
                args[args.index("--resume") + 1] == "ambient-owned-cursor-chat",
                "control: direct headless commit lost backend-scoped resume recipe")
        require(any("conventional commit message" in a for a in args),
                "control: actual staged-diff commit prompt did not reach executor")
    s = OwnedSession(binary, f)
    try:
        ordinary_control(s, f)
        f.set_control(action, marker=f"retained-{name}-owned-marker")
        # Trailing whitespace suppresses composer slash completion, so Enter
        # exercises dispatch instead of accepting a completion (A2 is separate).
        s.submit(f"/{name} ")
        record = f.wait_call(s, action)
        s.wait_join(0)
        s.redraw()
        require(f.marker.encode() in s.screen(),
                f"expectation A3: /{name} output absent after terminal redraw")
        f.assert_closed(record, s)
        s.quit_observed()
    finally:
        f.cleanup(s)


def cancelled_case(binary, root, name, quit_run=False, teacher=False):
    f = Fixture(root, teacher=teacher)
    action = "teacher" if teacher else ("generation" if name == "commit" else name)
    f.set_control(action, hold=True)
    s = OwnedSession(binary, f)
    try:
        text = "/learn distill --teacher fm synthetic-owned-task " if teacher else f"/{name} "
        s.submit(text)
        record = f.wait_call(s, action)
        require(exists(record["pid"]), "control: held executor already exited")
        require(record["leaf_pid"] is not None and exists(record["leaf_pid"]),
                "control: same-group owned descendant did not enter")
        leaf_end = time.monotonic() + WAIT
        while not any(r["pid"] == record["leaf_pid"]
                      for r in f.events(f.nonce, "leaf")) and time.monotonic() < leaf_end:
            s.read(.01)
        leaf_rows = [r for r in f.events(f.nonce, "leaf") if r["pid"] == record["leaf_pid"]]
        require(leaf_rows and leaf_rows[0]["pgid"] == record["pgid"],
                "control: actual same-group leaf readiness was not observed")
        if teacher:
            args = record["argv"]
            require(args[0] == "respond" and "--no-stream" in args,
                    f"control: wrong actual FM teacher grammar {args!r}")
            require(args[args.index("--model") + 1] == "system",
                    "control: teacher was not the explicit on-device recipe")
            require("--resume" not in args and "--save-transcript" not in args,
                    "control: stateless teacher unexpectedly resumed conversation")
            require(exists(record["ppid"]), "control: intermediate Abbey not alive")
        if quit_run:
            s.quit_observed()
        else:
            s.submit("queued-owned-never-contact")
            s.wait_output(b"queued (1)", timeout=WAIT)
            s.send(b"\x1b")
            # A3 old code remains interrupting; A5 old code reports exit130
            # while teacher's distinct group survives. Assertions precede cleanup.
            s.wait_join(130)
            s.wait_output(b"discarded 1 queued prompt", timeout=WAIT)
        f.assert_closed(record, s, include_parent=teacher and record["ppid"] != s.proc.pid)
        require(not any("queued-owned-never-contact" in " ".join(r["argv"])
                        for r in f.events()),
                "expectation: cancellation replayed queued provider work")
        if teacher:
            out, _ = f.cli(binary, ["learn", "sft"])
            require(not out.strip(), "expectation: cancelled teacher stored training pair")
        if not quit_run:
            ordinary_control(s, f)
            s.quit_observed()
    finally:
        f.cleanup(s)


def selected_backend_case(binary, root):
    f = Fixture(root)
    # Direct ambient Cursor is a positive control for the two distinct ids.
    out, _ = f.cli(binary, ["/memory"])
    require(b"chat: ambient-owned-cursor-chat" in out,
            "control: original Cursor id was not established")
    s = OwnedSession(binary, f)
    try:
        # Only ABI resolves among the next five backend probes; no real PATH.
        s.send(b"\x02")
        s.wait_output("backend → abi".encode(), timeout=WAIT)
        s.submit("/memory ")
        s.wait_join(0)
        s.redraw()
        mismatches = []
        if b"chat: selected-owned-abi-chat" not in s.screen():
            mismatches.append("local child did not use selected ABI chat owner")
        if b"chat: ambient-owned-cursor-chat" in s.screen():
            mismatches.append("local child adopted original provider's chat id")
        f.set_control(marker="selected-abi-reason-output")
        s.submit("/cot run synthetic-selected-owned-task ")
        record = f.wait_call(s, "generation")
        s.wait_join(0)
        if record["backend"] != "abi":
            mismatches.append(f"selected ABI dispatched to {record['backend']}")
        if not all(r["backend"] == "abi" for r in f.events(f.nonce)):
            mismatches.append("original cloud-shaped executor contacted")
        require("--live" not in record["argv"],
                "expectation A4: inherited thinking alias selected live ABI")
        cot = root / "state" / "cot" / "latest.md"
        if not cot.is_file() or f.marker not in cot.read_text():
            mismatches.append("successful local reason capture did not preserve CoT file")
        f.assert_closed(record, s)
        s.quit_observed()
        require(not mismatches, "expectation A4: " + "; ".join(mismatches))
    finally:
        f.cleanup(s)


def teacher_success_control(binary, root):
    f = Fixture(root, teacher=True)
    f.set_control("teacher", marker="owned-stateless-teacher-answer")
    original_chat = (root / "state" / "chat-id").read_bytes()
    out, err = f.cli(binary, ["learn", "distill", "--teacher", "fm", "synthetic-owned-task"])
    require(b"fm:system" in out and b"1 train_candidate record(s) stored" in err,
            "control: actual teacher did not store its successful candidate")
    records = f.events(f.nonce)
    require(len(records) == 1 and records[0]["action"] == "teacher",
            "control: explicit teacher triggered an unexpected provider")
    args = records[0]["argv"]
    require(args[0] == "respond" and "--no-stream" in args and
            "--resume" not in args and "--save-transcript" not in args,
            "control: FM stateless capture argv changed")
    out, _ = f.cli(binary, ["learn", "sft"])
    rows = [json.loads(line) for line in out.splitlines() if line.strip()]
    require(len(rows) == 1 and rows[0]["messages"][-1]["content"] == f.marker,
            "control: successful distillation did not export exact captured answer")
    require((root / "state" / "chat-id").read_bytes() == original_chat,
            "control: stateless teacher modified canonical conversation mirror")


def main():
    cases = {
        "output": [(f"A3-{n}-retained-output", lambda b, r, n=n: output_case(b, r, n))
                   for n in ("models", "status", "commit")],
        "cancel": [(f"A3-{n}-cancel-join", lambda b, r, n=n: cancelled_case(b, r, n))
                   for n in ("models", "status", "commit")],
        "quit": [(f"A3-{n}-quit-restore", lambda b, r, n=n: cancelled_case(b, r, n, True))
                 for n in ("models", "status", "commit")],
        "backend": [("A4-selected-cfg-local-child", selected_backend_case)],
        "teacher": [("A5-stateless-teacher-success-control", teacher_success_control),
                    ("A5-teacher-cancel-all-owned-groups", lambda b, r: cancelled_case(b, r, "", teacher=True)),
                    ("A5-teacher-quit-all-owned-groups", lambda b, r: cancelled_case(b, r, "", True, True))],
    }
    proofs, failures = [], []
    for name, test in cases[OPTIONS.mode]:
        try:
            with tempfile.TemporaryDirectory(prefix="abbey-owned-slash-pty-") as scratch:
                test(Path(OPTIONS.binary).resolve(), Path(scratch))
            proofs.append(dict(scenario=name, status="passed"))
        except Exception as error:
            failures.append(dict(scenario=name, status="failed", error=str(error)))
    print(json.dumps(dict(status="failed" if failures else "passed", mode=OPTIONS.mode,
                          scenarios=proofs, failures=failures), sort_keys=True))
    return 1 if failures else 0


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("binary")
    parser.add_argument("mode", choices=("output", "cancel", "quit", "backend", "teacher"))
    parser.add_argument("--state-env", required=True)
    parser.add_argument("--config-env", required=True)
    OPTIONS = parser.parse_args()
    sys.exit(main())
