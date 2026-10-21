"""Behavioral fail-closed tests; no GUI, app launch, or AX permission needed."""
import contextlib
import io
import json
from pathlib import Path
import struct
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import Mock, patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import prove_menu_icon_macos as prover


def snapshot():
    def menu(title, items):
        return {"title": title, "items": [{"title": item} for item in items]}
    return {
        "complete": True,
        "menus": [menu("Abbey", ["About Abbey", "Hide Abbey", "Quit Abbey"]),
                  menu("Edit", [labels[0] for labels in prover.EXPECTED_EDIT_ITEMS.values()]),
                  menu("Window", [labels[0] for labels in prover.EXPECTED_WINDOW_ITEMS.values()])],
        "windows": [{"title": "Abbey", "subrole": "AXStandardWindow"}],
    }


class Acceptance(unittest.TestCase):
    def run_prover(self, *, reason=None, registration="icon.icns", runner_error=None):
        with tempfile.TemporaryDirectory(prefix="menu-icon-test-") as temporary:
            root = Path(temporary)
            app = root / "Abbey.app"
            resources = app / "Contents" / "Resources"
            resources.mkdir(parents=True)
            (app / "Contents" / "MacOS").mkdir()
            (app / "Contents" / "MacOS" / "abbey-desktop").write_bytes(b"fixture")
            (app / "Contents" / "Info.plist").write_bytes(b"fixture")
            source = root / "source"
            source.mkdir()
            png = prover.PNG_SIGNATURE + b"\x00\x00\x00\x0dIHDR" + struct.pack(">II", 1024, 1024)
            icon = b"icns" + struct.pack(">I", 16 + len(png)) + b"ic10" + struct.pack(">I", 8 + len(png)) + png
            (source / "master.png").write_bytes(png)
            (source / "icon.icns").write_bytes(icon)
            (resources / "icon.icns").write_bytes(icon)
            plist = {"CFBundleIdentifier": "com.donaldfilimon.abbey",
                     "CFBundleName": "Abbey", "CFBundleDisplayName": "Abbey"}
            if registration is not None:
                plist["CFBundleIconFile"] = registration
            receipt = root / "receipt.json"
            process = Mock(pid=123)
            process.poll.return_value = None
            with contextlib.ExitStack() as stack:
                for name, value in {"OUT": root / "out", "RECEIPT": receipt,
                                    "SOURCE_ICONS": source, "SOURCE_ICON": source / "icon.icns",
                                    "SOURCE_MASTER": source / "master.png"}.items():
                    stack.enter_context(patch.object(prover, name, value))
                stack.enter_context(patch.object(sys, "argv", ["prover", "--app", str(app)]))
                stack.enter_context(patch.object(prover.platform, "system", return_value="Darwin"))
                stack.enter_context(patch.object(prover.signal, "signal"))
                stack.enter_context(patch.object(prover, "read_plist", return_value=plist))
                stack.enter_context(patch.object(prover, "compile_driver", side_effect=runner_error))
                stack.enter_context(patch.object(prover, "preflight", return_value=None))
                stack.enter_context(patch.object(prover.tempfile, "mkdtemp", return_value=str(root / "scratch")))
                stack.enter_context(patch.object(prover, "launch_app", return_value=(process, io.BytesIO())))
                stack.enter_context(patch.object(prover, "stop"))
                stack.enter_context(patch.object(prover, "settle", return_value=(snapshot(), [], reason)))
                stack.enter_context(contextlib.redirect_stdout(io.StringIO()))
                stack.enter_context(contextlib.redirect_stderr(io.StringIO()))
                code = prover.main()
            return code, json.loads(receipt.read_text())

    def test_complete_settled_fixture_passes(self):
        code, receipt = self.run_prover()
        self.assertEqual(code, 0)
        self.assertEqual(receipt["status"], "passed")

    def test_nonempty_unsettled_snapshot_is_unproven(self):
        code, receipt = self.run_prover(reason="ax_state_never_settled")
        self.assertEqual(code, 2)
        checks = [check for check in receipt["checks"] if check["id"].startswith("ax_")]
        self.assertEqual(len(checks), 5)
        self.assertTrue(all(check["status"] == "unproven" for check in checks))
        self.assertEqual(receipt["ax"]["reason"], "ax_state_never_settled")

    def test_wrong_or_missing_icon_registration_fails_even_with_matching_bytes(self):
        for registration in (None, "", "other.icns", "../icon.icns", "icons/icon.icns"):
            with self.subTest(registration=registration):
                code, receipt = self.run_prover(registration=registration)
                self.assertEqual(code, 1)
                checks = {check["id"]: check for check in receipt["checks"]}
                self.assertEqual(checks["icon_bundle_registration"]["status"], "failed")
                self.assertEqual(checks["icon_bundle_matches_source"]["status"], "passed")

    def test_extensionless_icon_registration_selects_same_resource(self):
        self.assertEqual(self.run_prover(registration="icon")[0], 0)

    def test_runner_error_after_successful_checks_is_recorded(self):
        for error in (OSError("fixture"), prover.ProofFailure("driver_build_failed")):
            with self.subTest(error=error):
                code, receipt = self.run_prover(runner_error=error)
                self.assertEqual(code, 1)
                self.assertEqual(receipt["status"], "failed")
                self.assertIn({"id": "runner_aborted", "status": "failed",
                               "assertion": receipt["failure"]}, receipt["checks"])

    def test_settle_deadline_retains_nonempty_observation_but_returns_reason(self):
        observed = snapshot()
        with patch.object(prover, "read_ax", return_value=(observed, None)), \
                patch.object(prover.time, "monotonic", side_effect=[0, 0, 0, 2]), \
                patch.object(prover.time, "sleep"):
            envelope, history, reason = prover.settle("unused", 123, deadline_seconds=1)
        self.assertEqual(envelope, observed)
        self.assertTrue(history)
        self.assertEqual(reason, "ax_state_never_settled")

    def test_empty_report_and_empty_required_sets_fail(self):
        self.assertEqual(prover.Report().status(), "failed")
        for expected in ({}, {"undo": ()}):
            with self.subTest(expected=expected), self.assertRaises(prover.ProofFailure):
                prover.check_menu_items({"items": []}, expected, "missing")
        with patch.object(prover, "EXPECTED_APP_MENU_ITEMS", {}):
            with self.assertRaises(prover.ProofFailure):
                prover.check_app_menu_items(snapshot()["menus"][0], "Abbey")
        with patch.object(prover, "EXPECTED_TOP_LEVEL_MENUS", ()):
            with self.assertRaises(prover.ProofFailure):
                prover.check_top_level_menus(snapshot()["menus"], "Abbey")

    def test_partial_or_limit_driver_output_cannot_be_accepted(self):
        for reason in ("ax_traversal_truncated", "ax_attribute_read_failed_-25204_AXTitle"):
            with self.subTest(reason=reason):
                result = subprocess.CompletedProcess([], 1, json.dumps(snapshot()).encode(), reason.encode())
                with patch.object(prover.subprocess, "run", return_value=result):
                    self.assertEqual(prover.read_ax("unused", 123), (None, reason))
        for complete in (None, False):
            partial = snapshot()
            partial["complete"] = complete
            result = subprocess.CompletedProcess([], 0, json.dumps(partial).encode(), b"")
            with patch.object(prover.subprocess, "run", return_value=result):
                self.assertEqual(prover.read_ax("unused", 123), (None, "ax_snapshot_incomplete"))


if __name__ == "__main__":
    unittest.main()
