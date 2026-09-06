#!/usr/bin/env python3
"""Own scratch processes and collect bounded native Routes acceptance evidence."""
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import secrets
import subprocess
import sys
import signal
from datetime import datetime, timezone
import tempfile
import time

from routes_assertions import WORKSPACES, ProofFailure, empty, inspect, populated, rejected, require

DESKTOP = Path(__file__).resolve().parents[1]
ROOT = DESKTOP.parent
OUT = DESKTOP / "target" / "routes-acceptance"


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def build_env():
    env = {key: os.environ[key] for key in ("HOME", "USER", "LOGNAME", "TMPDIR", "DEVELOPER_DIR", "SDKROOT") if key in os.environ}
    env.update(PATH=f"{Path.home()}/.cargo/bin:/usr/bin:/bin:/usr/sbin:/sbin:/opt/homebrew/bin", RUSTUP_TOOLCHAIN="nightly-2026-09-01", CC="/usr/bin/clang", CXX="/usr/bin/clang++")
    return env


def run_build(argv, cwd, env, log, timeout=1800):
    with log.open("wb") as output:
        try:
            result = subprocess.run(argv, cwd=cwd, env=env, stdout=output, stderr=subprocess.STDOUT, timeout=timeout)
        except subprocess.TimeoutExpired:
            raise ProofFailure("build_timeout") from None
    require(result.returncode == 0, "build_failed_" + log.stem)


def stop(process):
    if process.poll() is None:
        process.terminate()
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=5)


class Smoke:
    def __init__(self, scratch, driver, receipt):
        self.scratch = scratch
        self.driver = driver
        self.receipt = receipt
        self.processes = []
        self.bearer = secrets.token_hex(32)
        self.wrong = secrets.token_hex(32)
        self.secrets = [self.bearer, self.wrong]
        self.route_hashes = {}
        for name in ("daemon", "desktop", "empty", "work", "home", "config", "cache"):
            (scratch / name).mkdir(mode=0o700)
        for name in ("daemon", "desktop", "empty"):
            (scratch / name / "config.toml").write_text("")
        records = []
        for i in range(55):
            records.append(dict(
                ts=f"2026-09-01T12:00:{i:02}Z",
                cwd=WORKSPACES[i % 3], persona="Abbey", role="max",
                model="fixture-model", confidence=0.82,
                reason=(f"ROUTE_ROW_{i:03} /private/SECRET_UNIX_PATH "
                        "C:\\private\\SECRET_WINDOWS_PATH ~/SECRET_HOME_PATH"),
                tools=["route-tool"], correlation="route-correlation",
                stage="route-stage", alternate="fixture-alternate",
                fallback="fixture-fallback",
            ))
        for name, rows in (("daemon", records), ("empty", []), ("desktop", [dict(records[0], reason="LOCAL_FALLBACK_CANARY")])):
            path = scratch / name / "route.jsonl"
            path.write_text("".join(json.dumps(row) + "\n" for row in rows))
            path.chmod(0o600)
            self.route_hashes[path] = sha(path)

    def env(self, state, socket, bearer):
        # No inherited provider, model, credential, or personal-edition variables.
        return dict(
            PATH="/usr/bin:/bin:/usr/sbin:/sbin",
            HOME=str(self.scratch / "home"),
            XDG_CONFIG_HOME=str(self.scratch / "config"),
            XDG_CACHE_HOME=str(self.scratch / "cache"),
            XDG_STATE_HOME=str(self.scratch / "home"),
            ABBEY_STATE_DIR=str(self.scratch / state),
            ABBEY_CONFIG=str(self.scratch / state / "config.toml"),
            ABBEYD_SOCKET_PATH=str(socket), ABBEYD_BEARER_TOKEN=bearer,
            ABBEY_MEMORY_BACKEND="sqlite",
        )

    def launch(self, binary, env, label):
        with (self.scratch / (label + ".log")).open("wb") as log:
            proc = subprocess.Popen([str(binary)], cwd=self.scratch / "work", env=env, stdout=log, stderr=log)
        self.processes.append(proc)
        return proc

    def daemon(self, state):
        socket = self.scratch / (state + ".sock")
        proc = self.launch(ROOT / "target/debug/abbeyd", self.env(state, socket, self.bearer), state)
        deadline = time.monotonic() + 15
        while not socket.is_socket():
            require(proc.poll() is None, "scratch_daemon_exited")
            require(time.monotonic() < deadline, "scratch_socket_timeout")
            time.sleep(0.05)
        require(socket.stat().st_mode & 0o077 == 0, "scratch_socket_permissions")
        return proc, socket

    def desktop(self, socket, bearer):
        return self.launch(DESKTOP / "target/debug/abbey-desktop", self.env("desktop", socket, bearer), "desktop")

    def ax(self, proc, action="snapshot", *args):
        require(proc.poll() is None, "owned_desktop_exited")
        try:
            result = subprocess.run([str(self.driver), str(proc.pid), action, *args], capture_output=True, timeout=15)
        except subprocess.TimeoutExpired:
            raise ProofFailure("ax_command_timeout") from None
        if result.returncode:
            # Only this static driver diagnostic may be surfaced, never AX output.
            error = result.stderr.decode(errors="replace").strip()
            allowed = {"ax_window_missing", "accessibility_permission_required", "ax_attribute_read_failed", "ax_attribute_inventory_failed", "ax_traversal_truncated", "ax_control_missing_or_ambiguous", "ax_press_failed", "ax_limit_option_missing", "owned_process_unavailable"}
            numeric_inventory_error = re.fullmatch(r"ax_attribute_inventory_failed_-?[0-9]+_depth_[0-9]+", error)
            raise ProofFailure(error if error in allowed or numeric_inventory_error else "ax_driver_failed")
        try:
            snapshots = json.loads(result.stdout)
        except (ValueError, UnicodeError):
            raise ProofFailure("ax_json_invalid") from None
        require(isinstance(snapshots, list) and bool(snapshots), "ax_no_snapshots")
        for snapshot in snapshots:
            inspect(snapshot, self.secrets)
        return snapshots[-1]

    def wait(self, proc, assertion):
        deadline = time.monotonic() + 20
        last = "ax_window_missing"
        while time.monotonic() < deadline:
            try:
                snapshot = self.ax(proc)
                assertion(snapshot)
                return
            except ProofFailure as error:
                last = str(error)
                # Secrecy, incomplete traversal, missing permission and process loss
                # are fatal, never retried into a later clean snapshot.
                if last not in {"owned_process_unavailable", "ax_window_missing", "daemon_connection_missing", "route_rows_or_order", "route_summary", "error_message_missing", "empty_message_missing", "error_has_stale_rows", "empty_has_stale_rows"}:
                    raise
            time.sleep(0.15)
        raise ProofFailure("timeout_" + last)

    def connected(self, proc):
        self.wait(proc, lambda snap: require("abbeyd (authenticated Unix socket)" in inspect(snap, self.secrets), "daemon_connection_missing"))
        self.ax(proc, "routes")

    def mark(self, scenario):
        self.receipt["completed_scenarios"].append(scenario)
        print("PASS " + scenario, flush=True)

    def execute(self):
        daemon, socket = self.daemon("daemon")
        app = self.desktop(socket, self.bearer)
        self.connected(app)
        self.wait(app, lambda snap: populated(snap, 25, self.secrets))
        self.mark("authenticated-25")
        for limit in (10, 50):
            self.ax(app, "limit", str(limit))
            self.wait(app, lambda snap: populated(snap, limit, self.secrets))
            self.mark("limit-" + str(limit))
        wrong = self.desktop(socket, self.wrong)
        self.wait(wrong, lambda snap: rejected(snap, self.secrets, "Request rejected"))
        stop(wrong)
        self.mark("authentication-rejection")
        stop(daemon)
        self.ax(app, "limit", "10")
        self.wait(app, lambda snap: rejected(snap, self.secrets, "Cannot reach Abbey"))
        stop(app)
        self.mark("daemon-loss-no-fallback")
        _, empty_socket = self.daemon("empty")
        blank = self.desktop(empty_socket, self.bearer)
        self.connected(blank)
        self.wait(blank, lambda snap: empty(snap, self.secrets))
        stop(blank)
        self.mark("empty-log")
        self.verify_files()
        self.mark("route-files-unchanged")

    def verify_files(self):
        for path, expected in self.route_hashes.items():
            require(path.is_file() and sha(path) == expected, "route_file_mutated")

    def close(self):
        for proc in reversed(self.processes):
            stop(proc)
        self.verify_files()


def main():
    def interrupted(_number, _frame):
        raise ProofFailure("runner_interrupted")
    signal.signal(signal.SIGTERM, interrupted)
    signal.signal(signal.SIGINT, interrupted)
    os.umask(0o077)
    OUT.mkdir(parents=True, exist_ok=True)
    receipt = dict(schema=1, started_at=datetime.now(timezone.utc).isoformat(), status="failed", platform=platform.system(), architecture=platform.machine(), completed_scenarios=[], gates={"desktop": "not_run", "root": "not_run"})
    receipt["revision"] = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip()
    receipt["dirty"] = bool(subprocess.check_output(["git", "status", "--porcelain"], cwd=ROOT))
    smoke = None
    try:
        require(platform.system() == "Darwin", "macos_required")
        env = build_env()
        driver = OUT / "routes-ax"
        print("== compile native Accessibility driver ==", flush=True)
        run_build(["/usr/bin/xcrun", "swiftc", "-warnings-as-errors", str(DESKTOP / "scripts/routes-ax.swift"), "-o", str(driver)], DESKTOP, env, OUT / "driver-build.log", 120)
        receipt["artifacts"] = {"driver": sha(driver)}
        check = subprocess.run([str(driver), "preflight"], capture_output=True, timeout=10)
        require(check.returncode == 0, "accessibility_or_interactive_gui_required")
        print("== build scratch daemon and bundled desktop ==", flush=True)
        run_build(["cargo", "build", "--locked", "--bin", "abbeyd"], ROOT, env, OUT / "daemon-build.log")
        receipt["artifacts"]["abbeyd"] = sha(ROOT / "target/debug/abbeyd")
        run_build(["bun", "run", "build"], DESKTOP, env, OUT / "frontend-build.log")
        run_build(["bun", "run", "tauri", "build", "--debug", "--no-bundle"], DESKTOP, env, OUT / "desktop-build.log")
        receipt["artifacts"]["desktop"] = sha(DESKTOP / "target/debug/abbey-desktop")
        receipt["frontend"] = {str(path.relative_to(DESKTOP / "dist")): sha(path) for path in sorted((DESKTOP / "dist").rglob("*")) if path.is_file()}
        with tempfile.TemporaryDirectory(prefix="abbey-routes-", dir="/tmp") as temporary:
            try:
                smoke = Smoke(Path(temporary), driver, receipt)
                smoke.execute()
            finally:
                if smoke is not None:
                    smoke.close()
        receipt["status"] = "passed"
    except (ProofFailure, subprocess.SubprocessError, OSError):
        error = sys.exc_info()[1]
        receipt["failure"] = str(error) if isinstance(error, ProofFailure) else "runner_system_failure"
        print("FAIL " + receipt["failure"], file=sys.stderr)
    finally:
        receipt["finished_at"] = datetime.now(timezone.utc).isoformat()
        (OUT / "receipt.json").write_text(json.dumps(receipt, indent=2) + "\n")
        print("Receipt: desktop/target/routes-acceptance/receipt.json", flush=True)
    return 0 if receipt["status"] == "passed" else 1


if __name__ == "__main__":
    sys.exit(main())
