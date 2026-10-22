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

Bundle selection: the bundle's own identifier decides which edition is
proved, never its name and never its position in a directory listing. Both
editions stage into the same directory, so the old pick
(`sorted(glob("*.app"))[-1]`) proved whatever happened to be there: with
only the personal bundle staged it proved the *personal* app while the gate
believed it proved the shipped one, and with both staged it happened to
land on the safe one — an intermittent, silent wrong-edition proof. The
expectation is now explicit: `--expect-edition` says which edition must be
proved, and the run fails loudly when the bundle on disk is not that
edition.

Usage:
    ./scripts/prove-packaged-macos.sh [--expect-edition safe|personal]
    ./scripts/prove-packaged-macos.sh --app PATH [--expect-edition safe|personal]
    ./scripts/prove-packaged-macos.sh [--skip-build]
"""
from datetime import datetime, timezone
from pathlib import Path
import platform
import plistlib
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

# The bundle identity decides the edition. The path never does: both
# editions' bundles are staged into the same directory, and a directory
# listing is not a proof.
EDITIONS_BY_IDENTIFIER = {
    "com.donaldfilimon.abbey": "safe",
    "com.donaldfilimon.abbey-personal": "personal",
}

# The shipped default. The personal bundle is proved only when it was
# actually staged *and* explicitly asked for.
DEFAULT_EDITION = "safe"

# Where `release-macos.sh` stages both editions' bundles.
STAGED_BUNDLE_DIR = "target/release/bundle/macos"


def bundle_identifier(app_path):
    """The bundle's own identifier, or "" when the plist cannot be read."""
    plist_path = Path(app_path) / "Contents" / "Info.plist"
    try:
        with plist_path.open("rb") as handle:
            plist = plistlib.load(handle)
    except (OSError, ValueError, plistlib.InvalidFileException):
        return ""
    identifier = plist.get("CFBundleIdentifier", "")
    return identifier if isinstance(identifier, str) else ""


def staged_bundles(directory):
    """Every staged `.app`, with the edition its own identifier declares.

    A bundle whose identifier is unknown is reported with `edition: None`:
    it is never silently treated as the expected edition.
    """
    bundles = []
    for candidate in sorted(Path(directory).glob("*.app")):
        identifier = bundle_identifier(candidate)
        bundles.append({
            "path": candidate,
            "identifier": identifier,
            "edition": EDITIONS_BY_IDENTIFIER.get(identifier),
        })
    return bundles


def select_bundle(directory, expect_edition):
    """The one staged bundle that *is* the expected edition.

    Fails loudly rather than falling back to some other bundle: proving the
    wrong edition's app is worse than proving nothing.
    """
    require(expect_edition in set(EDITIONS_BY_IDENTIFIER.values()),
            "unknown_expected_edition")
    matches = [bundle for bundle in staged_bundles(directory)
               if bundle["edition"] == expect_edition]
    require(bool(matches), "no_staged_bundle_for_expected_edition")
    require(len(matches) == 1, "ambiguous_staged_bundle_for_expected_edition")
    return matches[0]


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
        expect_edition = DEFAULT_EDITION
        skip_build = False
        argv = sys.argv[1:]
        i = 0
        while i < len(argv):
            if argv[i] == "--app":
                require(i + 1 < len(argv), "app_path_required")
                app_path = Path(argv[i + 1]).expanduser().resolve()
                i += 1
            elif argv[i] == "--expect-edition":
                require(i + 1 < len(argv), "expect_edition_required")
                expect_edition = argv[i + 1]
                i += 1
            elif argv[i] == "--skip-build":
                skip_build = True
            else:
                raise ProofFailure("unknown_argument")
            i += 1
        require(expect_edition in set(EDITIONS_BY_IDENTIFIER.values()),
                "unknown_expected_edition")
        if app_path is None:
            # No explicit path: pick the staged bundle *by the edition its
            # identifier declares*. The directory-order pick this replaced
            # proved whatever happened to be staged, which is not an
            # expectation.
            bundle = select_bundle(base.ROOT / STAGED_BUNDLE_DIR, expect_edition)
            app_path = bundle["path"]
            identifier = bundle["identifier"]
            edition = bundle["edition"]
        else:
            identifier = bundle_identifier(app_path)
            require(identifier in EDITIONS_BY_IDENTIFIER, "bundle_identifier_unknown")
            edition = EDITIONS_BY_IDENTIFIER[identifier]
            require(edition == expect_edition, "bundle_edition_mismatch")
        print(f"bundle:  {app_path}")
        print(f"edition: {edition} ({identifier}) — expected {expect_edition}")
        receipt["bundle"] = {
            "path": str(app_path),
            "identifier": identifier,
            "edition": edition,
            "expected_edition": expect_edition,
        }
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
        print("Receipt: target/routes-acceptance/packaged-receipt.json", flush=True)
    return 0 if receipt["status"] == "passed" else 1


if __name__ == "__main__":
    sys.exit(main())
