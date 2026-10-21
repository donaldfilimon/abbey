#!/usr/bin/env python3
"""Run the canonical Routes acceptance scenarios against the *packaged* .app.

The unpackaged prover (`prove_routes_macos.py`) drives the debug developer
binary. This prover drives the binary inside a real bundle layout — the exact
artifact `./scripts/release-macos.sh` produces — so the app is exercised as
installed: resource lookup from the bundle, the packaged Info.plist, and the
hardened-runtime signature, against a scratch abbeyd with the same native
Accessibility driver and the same scenario table.

Signing boundary: the staging bundle is ad-hoc signed by release-macos.sh. This
prover verifies that signature is present and internally consistent; it does
NOT measure notarized first-launch behavior (Gatekeeper, quarantine, `open`),
which needs the owner's Developer ID credentials.

Usage:
    ./scripts/prove-packaged-macos.sh [--app PATH] [--skip-build]
"""
from datetime import datetime, timezone
from pathlib import Path
import platform
import shutil
import signal
import sys

import prove_routes_macos as base
from routes_assertions import ProofFailure, require
import subprocess

HARNESS_FILES = (
    "prove-routes-macos.sh", "prove_routes_macos.py", "routes_assertions.py",
    "routes-ax.swift", "release-macos.sh", "prove-packaged-macos.sh",
    "prove_packaged_macos.py",
)


class PackagedSmoke(base.Smoke):
    """Same scratch setup and scenario script; the desktop binary is bundled."""

    def __init__(self, scratch, driver, receipt, app_binary):
        super().__init__(scratch, driver, receipt)
        self.app_binary = app_binary


    def desktop(self, socket, bearer):
        # Launch the packaged binary by its bundle-internal real path. Bundle
        # resource resolution and the packaged Info.plist make this a
        # materially different process than the raw developer binary.
        return self.launch(self.app_binary, self.env("desktop", socket, bearer), "desktop")


def main():
    def interrupted(_number, _frame):
        raise ProofFailure("runner_interrupted")
    signal.signal(signal.SIGTERM, interrupted)
    signal.signal(signal.SIGINT, interrupted)
    base.OUT.mkdir(parents=True, exist_ok=True)
    receipt = dict(
        schema=1, kind="packaged-routes-acceptance",
        started_at=datetime.now(timezone.utc).isoformat(), status="failed",
        platform=platform.system(), architecture=platform.machine(),
        completed_scenarios=[],
    )
    receipt["revision"] = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=base.ROOT, text=True).strip()
    receipt["dirty"] = bool(subprocess.check_output(["git", "status", "--porcelain"], cwd=base.ROOT))
    receipt["harness"] = {name: base.sha(base.DESKTOP / "scripts" / name) for name in HARNESS_FILES}
    smoke = None
    scratch_path = None
    try:
        require(platform.system() == "Darwin", "macos_required")
        app_path = None
        skip_build = False
        argv = sys.argv[1:]
        i = 0
        while i < len(argv):
            if argv[i] == "--app":
                app_path = Path(argv[i + 1]).resolve()
                i += 1
            elif argv[i] == "--skip-build":
                skip_build = True
            else:
                raise ProofFailure("unknown_argument")
            i += 1
        if app_path is None:
            candidates = sorted((base.DESKTOP / "target/release/bundle/macos").glob("*.app"))
            require(bool(candidates), "no_staged_app_built_yet")
            app_path = candidates[-1]
        app_binary = app_path / "Contents/MacOS/abbey-desktop"
        require(app_binary.is_file(), "packaged_binary_missing")
        # The staged app must be at least ad-hoc signed: staging verifies the
        # pipeline; a fully unsigned bundle fails here instead of unstaging.
        verify = subprocess.run(["/usr/bin/codesign", "--verify", "--strict", str(app_path)],
                                capture_output=True, timeout=30)
        require(verify.returncode == 0, "packaged_bundle_signature_invalid")
        signed = subprocess.run(["/usr/bin/codesign", "-dv", "--entitlements", "-", str(app_path)],
                                capture_output=True, timeout=30)
        details = signed.stderr.decode(errors="replace")
        signature_lines = [line.strip() for line in details.splitlines()
                           if "flags=" in line or "Identifier=" in line]
        receipt["codesign"] = signature_lines
        print("signature: " + (" ; ".join(signature_lines) or "<none>"), flush=True)
        receipt["artifacts"] = {
            "packaged_app_binary_sha256": base.sha(app_binary),
            "app_path": str(app_path),
        }
        driver = base.OUT / "routes-ax"
        if not (skip_build and driver.is_file()):
            env = base.build_env()
            print("== compile native Accessibility driver ==", flush=True)
            base.run_build(["/usr/bin/xcrun", "swiftc", "-warnings-as-errors",
                            str(base.DESKTOP / "scripts/routes-ax.swift"), "-o", str(driver)],
                           base.DESKTOP, env, base.OUT / "packaged-driver-build.log", 120)
        receipt["artifacts"]["driver_sha256"] = base.sha(driver)
        check = subprocess.run([str(driver), "preflight"], capture_output=True, timeout=10)
        require(check.returncode == 0, "accessibility_or_interactive_gui_required")
        print("== scratch daemon (debug abbeyd) ==", flush=True)
        env = base.build_env()
        daemon_bin = base.ROOT / "target/debug/abbeyd"
        if not skip_build or not daemon_bin.is_file():
            base.run_build(["cargo", "build", "--locked", "--bin", "abbeyd"],
                           base.ROOT, env, base.OUT / "packaged-daemon-build.log")
        require(daemon_bin.is_file(), "scratch_daemon_binary_missing")
        receipt["artifacts"]["abbeyd_sha256"] = base.sha(daemon_bin)
        temporary = base.tempfile.mkdtemp(prefix="abbey-packaged-routes-", dir="/tmp")
        scratch_path = Path(temporary)
        smoke = PackagedSmoke(scratch_path, driver, receipt, app_binary)
        smoke.execute()
        require(receipt["completed_scenarios"] == [
            "authenticated-25", "limit-10", "limit-50", "authentication-rejection",
            "daemon-loss-no-fallback", "empty-log", "route-files-unchanged",
        ], "incomplete_scenario_sequence")
        require(all(base.sha(base.DESKTOP / "scripts" / name) == digest
                    for name, digest in receipt["harness"].items()), "harness_changed_during_smoke")
        receipt["status"] = "passed"
    except (ProofFailure, subprocess.SubprocessError, OSError):
        error = sys.exc_info()[1]
        receipt["failure"] = str(error) if isinstance(error, ProofFailure) else "runner_system_failure"
        print("FAIL " + receipt["failure"], file=sys.stderr)
    finally:
        if smoke is not None:
            smoke.close()
        if scratch_path is not None:
            if receipt["status"] == "passed":
                shutil.rmtree(scratch_path, ignore_errors=True)
            else:
                receipt["scratch_diagnostics"] = str(scratch_path)
        receipt["finished_at"] = datetime.now(timezone.utc).isoformat()
        (base.OUT / "packaged-receipt.json").write_text(base.json.dumps(receipt, indent=2) + "\n")
        print("Receipt: desktop/target/routes-acceptance/packaged-receipt.json", flush=True)
    return 0 if receipt["status"] == "passed" else 1


if __name__ == "__main__":
    sys.exit(main())
