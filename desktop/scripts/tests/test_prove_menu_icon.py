"""Static proof checks for the packaged menu/icon prover. Never opens a window.

The prover itself needs a window server and the signed bundles, so this suite
asserts the parts that can be checked without either: that the expected menu
labels and the icon-identity rules are still declared, and that the pure
assertion helpers actually fail on a missing item instead of passing anything.
"""

import contextlib
import io
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch


SCRIPTS = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(SCRIPTS))

import prove_menu_icon_macos as prover


def menu(title, items):
    return {"title": title, "role": "AXMenuBarItem",
            "items": [{"title": item, "role": "AXMenuItem", "enabled": True}
                      for item in items]}


# The menu structure exactly as macOS reports it for the packaged app: the
# OS supplies its own entries (AutoFill, Close All, the Apple menu) on top of
# the entries the app defines.
OBSERVED_APP_MENU = ["About Abbey", "Services", "Hide Abbey", "Hide Others",
                     "Show All", "Quit Abbey", "Quit and Keep Windows"]
OBSERVED_EDIT_MENU = ["Undo", "Redo", "Cut", "Copy", "Paste", "Select All",
                      "AutoFill", "Start Dictation…", "Emoji & Symbols"]
OBSERVED_WINDOW_MENU = ["Toggle Full Screen", "Minimize", "Zoom",
                         "Bring All to Front", "Close Window", "Close All"]


class DeclaredExpectations(unittest.TestCase):
    """The expected labels are explicit and are not weakened silently."""

    def test_edit_items_are_declared_explicitly(self):
        self.assertEqual(prover.EXPECTED_EDIT_ITEMS, {
            "undo": ("undo",),
            "redo": ("redo",),
            "cut": ("cut",),
            "copy": ("copy",),
            "paste": ("paste",),
            "select_all": ("select all",),
        })

    def test_window_items_declare_the_os_labels_and_their_alternates(self):
        self.assertEqual(prover.EXPECTED_WINDOW_ITEMS, {
            "minimize": ("minimize",),
            "zoom": ("zoom", "maximize"),
            "fullscreen": ("toggle full screen", "enter full screen", "full screen"),
            "close": ("close window", "close"),
        })

    def test_top_level_menus_are_edit_and_window(self):
        self.assertEqual(prover.EXPECTED_TOP_LEVEL_MENUS, ("edit", "window"))

    def test_editions_are_bound_to_the_bundle_identifier(self):
        self.assertEqual(prover.EDITIONS_BY_IDENTIFIER, {
            "com.donaldfilimon.abbey": {"edition": "safe", "product_name": "Abbey"},
            "com.donaldfilimon.abbey-personal": {"edition": "personal",
                                                 "product_name": "Abbey Personal"},
        })

    def test_prose_labels_are_documented_for_the_receipt(self):
        self.assertEqual(prover.LABEL_PROSE["zoom"], "maximize")
        self.assertEqual(prover.LABEL_PROSE["fullscreen"], "fullscreen")
        self.assertEqual(prover.LABEL_PROSE["close"], "close")
        self.assertEqual(prover.LABEL_PROSE["select_all"], "select all")


class MenuAssertions(unittest.TestCase):
    """The pure helpers reject a menu that is missing a required entry."""

    def test_observed_menus_satisfy_every_expectation(self):
        menus = [menu("Apple", ["About This Mac"]),
                 menu("Abbey", OBSERVED_APP_MENU),
                 menu("Edit", OBSERVED_EDIT_MENU),
                 menu("Window", OBSERVED_WINDOW_MENU)]
        detail = prover.check_top_level_menus(menus, "Abbey")
        self.assertEqual(detail["app_menu_title"], "Abbey")
        prover.check_app_menu_items(prover.find_menu(menus, "Abbey"), "Abbey")
        prover.check_menu_items(prover.find_menu(menus, "Edit"),
                                prover.EXPECTED_EDIT_ITEMS, "ax_edit_menu_items_missing")
        prover.check_menu_items(prover.find_menu(menus, "Window"),
                                prover.EXPECTED_WINDOW_ITEMS, "ax_window_menu_items_missing")

    def test_missing_app_menu_fails(self):
        menus = [menu("Edit", OBSERVED_EDIT_MENU), menu("Window", OBSERVED_WINDOW_MENU)]
        with self.assertRaisesRegex(prover.ProofFailure, "ax_app_menu_missing"):
            prover.check_top_level_menus(menus, "Abbey")

    def test_missing_edit_or_window_menu_fails(self):
        menus = [menu("Abbey", OBSERVED_APP_MENU), menu("Edit", OBSERVED_EDIT_MENU)]
        with self.assertRaisesRegex(prover.ProofFailure, "ax_top_level_menu_missing_window"):
            prover.check_top_level_menus(menus, "Abbey")
        menus = [menu("Abbey", OBSERVED_APP_MENU), menu("Window", OBSERVED_WINDOW_MENU)]
        with self.assertRaisesRegex(prover.ProofFailure, "ax_top_level_menu_missing_edit"):
            prover.check_top_level_menus(menus, "Abbey")

    def test_missing_edit_item_fails(self):
        trimmed = [item for item in OBSERVED_EDIT_MENU if item != "Select All"]
        edit = menu("Edit", trimmed)
        with self.assertRaisesRegex(prover.ProofFailure, "ax_edit_menu_items_missing"):
            prover.check_menu_items(edit, prover.EXPECTED_EDIT_ITEMS, "ax_edit_menu_items_missing")
        self.assertEqual(prover.missing_items(edit, prover.EXPECTED_EDIT_ITEMS),
                         {"select_all": ["select all"]})

    def test_missing_window_item_fails(self):
        trimmed = [item for item in OBSERVED_WINDOW_MENU if item != "Zoom"]
        window = menu("Window", trimmed)
        with self.assertRaisesRegex(prover.ProofFailure, "ax_window_menu_items_missing"):
            prover.check_menu_items(window, prover.EXPECTED_WINDOW_ITEMS,
                                    "ax_window_menu_items_missing")
        self.assertEqual(prover.missing_items(window, prover.EXPECTED_WINDOW_ITEMS),
                         {"zoom": ["zoom", "maximize"]})

    def test_app_menu_without_the_standard_entries_fails(self):
        app = menu("Abbey", ["About Abbey"])
        with self.assertRaisesRegex(prover.ProofFailure, "ax_app_menu_items_missing"):
            prover.check_app_menu_items(app, "Abbey")

    def test_labels_are_compared_without_case_or_whitespace(self):
        edit = menu("Edit", ["undo", "  Redo ", "CUT", "Copy", "Paste", "select   all"])
        prover.check_menu_items(edit, prover.EXPECTED_EDIT_ITEMS, "ax_edit_menu_items_missing")


class WindowTitleAssertions(unittest.TestCase):
    def windows(self, *titles):
        return [{"title": title, "role": "AXWindow", "subrole": "AXStandardWindow"}
                for title in titles]

    def test_settled_title_must_name_this_edition(self):
        detail = prover.check_window_title(self.windows("Abbey Personal"),
                                            "Abbey Personal", ["Abbey"])
        self.assertEqual(detail["matched_title"], "Abbey Personal")

    def test_the_other_editions_name_is_rejected(self):
        with self.assertRaisesRegex(prover.ProofFailure, "ax_window_titles_other_edition"):
            prover.check_window_title(self.windows("Abbey"), "Abbey Personal", ["Abbey"])

    def test_a_wrong_title_is_rejected(self):
        with self.assertRaisesRegex(prover.ProofFailure, "ax_window_title_mismatch"):
            prover.check_window_title(self.windows("Untitled"), "Abbey", ["Abbey Personal"])

    def test_a_missing_window_is_rejected(self):
        with self.assertRaisesRegex(prover.ProofFailure, "ax_standard_window_missing"):
            prover.check_window_title([], "Abbey", ["Abbey Personal"])


class IconAssertions(unittest.TestCase):
    def test_png_dimensions_are_read_from_the_ihdr(self):
        header = b"\x89PNG\r\n\x1a\n" + b"\x00\x00\x00\x0dIHDR" + (1024).to_bytes(4, "big") \
            + (1024).to_bytes(4, "big") + b"\x00" * 20
        self.assertEqual(prover.png_dimensions(header), (1024, 1024))
        self.assertIsNone(prover.png_dimensions(b"not a png"))
        self.assertIsNone(prover.png_dimensions(header[:20]))

    def test_icns_entries_expose_their_embedded_png_size(self):
        png = b"\x89PNG\r\n\x1a\n" + b"\x00\x00\x00\x0dIHDR" + (1024).to_bytes(4, "big") \
            + (1024).to_bytes(4, "big") + b"\x00" * 20
        body = b"icns" + (8 + len(png)).to_bytes(4, "big") + b"ic10" \
            + (8 + len(png)).to_bytes(4, "big") + png
        entries = prover.icns_entries(body)
        self.assertEqual(entries, [{"type": "ic10", "length": 8 + len(png),
                                    "png_width": 1024, "png_height": 1024}])
        prover.check_icon_chain(entries)

    def test_a_chain_without_a_1024_representation_fails(self):
        entries = [{"type": "ic09", "length": 10, "png_width": 512, "png_height": 512}]
        with self.assertRaisesRegex(prover.ProofFailure, "icon_chain_missing_1024"):
            prover.check_icon_chain(entries)

    def test_a_placeholder_resource_in_the_bundle_fails(self):
        with self.assertRaisesRegex(prover.ProofFailure, "icon_placeholder_in_bundle"):
            prover.check_bundle_resources({"icon.png": "0" * 64}, prover.SOURCE_ICONS)

    def test_the_registered_icon_must_be_the_one_whose_bytes_are_checked(self):
        detail = prover.check_icon_registration({"CFBundleIconFile": "icon.icns"})
        self.assertEqual(detail["selected_resource"], "icon.icns")
        with self.assertRaisesRegex(prover.ProofFailure, "bundle_icon_registration_mismatch"):
            prover.check_icon_registration({"CFBundleIconFile": "other.icns"})

    def test_the_source_icon_and_master_are_the_declared_paths(self):
        self.assertEqual(prover.SOURCE_ICON,
                         prover.DESKTOP / "src-tauri" / "icons" / "icon.icns")
        self.assertEqual(prover.SOURCE_MASTER,
                         prover.DESKTOP / "src-tauri" / "icons" / "master.png")

    def test_the_prover_compares_the_bundle_icon_with_the_source_icon(self):
        source = Path(prover.__file__).read_text()
        self.assertIn("icon_bundle_matches_source", source)
        self.assertIn("sha256(SOURCE_ICON)", source)
        self.assertIn("icon_master_is_1024", source)
        self.assertIn("icon_chain_has_1024", source)
        self.assertIn("icon_no_placeholder_in_bundle", source)
        self.assertIn("icon_bundle_registration", source)


class UnprovenPath(unittest.TestCase):
    """A missing Accessibility read is reported, never upgraded to a pass."""

    def test_settle_reports_the_raw_driver_failure(self):
        with patch.object(prover, "read_ax",
                          return_value=(None, "accessibility_permission_required")):
            envelope, history, reason = prover.settle("driver", 123, deadline_seconds=1)
        self.assertIsNone(envelope)
        self.assertEqual(reason, "accessibility_permission_required")
        self.assertEqual(history, [])

    def test_an_unproven_check_keeps_the_receipt_unproven(self):
        report = prover.Report()
        report.passed("icon_bundle_matches_source", sha256="0" * 64)
        report.unproven("ax_menu_bar", "accessibility_permission_required")
        self.assertEqual(report.status(), "unproven")

    def test_a_failed_check_fails_the_receipt(self):
        report = prover.Report()
        report.passed("icon_bundle_matches_source", sha256="0" * 64)
        report.failed("ax_window_title", "ax_window_title_mismatch")
        self.assertEqual(report.status(), "failed")

    def test_the_exit_codes_separate_a_pass_from_a_failure_and_an_unproven_run(self):
        source = Path(prover.__file__).read_text()
        self.assertIn('if receipt["status"] == "passed":', source)
        self.assertIn('if receipt["status"] == "unproven":', source)
        self.assertIn("return 2", source)
        self.assertIn("return 1", source)


@unittest.skipUnless(sys.platform == "darwin", "macOS bundle fixtures")
class UnprovenRuntimePath(unittest.TestCase):
    """The real runner, with the Accessibility read refused.

    The bundle is a fixture, the driver is never built, and no app is
    launched: the run must deliver the icon and bundle checks, mark every AX
    check UNPROVEN with the raw reason, and exit with the unproven code.
    """

    def bundle(self, root):
        bundle = root / "Abbey.app"
        (bundle / "Contents/MacOS").mkdir(parents=True)
        (bundle / "Contents/Resources").mkdir(parents=True)
        (bundle / "Contents/MacOS/abbey-desktop").write_bytes(b"")
        (bundle / "Contents/Resources/icon.icns").write_bytes(prover.SOURCE_ICON.read_bytes())
        (bundle / "Contents/Info.plist").write_text(
            '<?xml version="1.0" encoding="UTF-8"?>\n'
            '<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" '
            '"http://www.apple.com/DTDs/PropertyList-1.0.dtd">\n'
            '<plist version="1.0"><dict>'
            "<key>CFBundleIdentifier</key><string>com.donaldfilimon.abbey</string>"
            "<key>CFBundleName</key><string>Abbey</string>"
            "<key>CFBundleDisplayName</key><string>Abbey</string>"
            "<key>CFBundleIconFile</key><string>icon.icns</string>"
            "</dict></plist>\n")
        return bundle

    def test_without_accessibility_the_icon_checks_deliver_and_ax_stays_unproven(self):
        with tempfile.TemporaryDirectory() as root:
            root = Path(root)
            bundle = self.bundle(root)
            receipt_path = root / "receipt.json"
            argv = sys.argv
            sys.argv = ["prove_menu_icon_macos.py", "--app", str(bundle)]
            try:
                with patch.object(prover, "RECEIPT", receipt_path), \
                        patch.object(prover, "compile_driver", lambda receipt: None), \
                        patch.object(prover, "preflight",
                                     lambda driver: "accessibility_permission_required"), \
                        contextlib.redirect_stdout(io.StringIO()), \
                        contextlib.redirect_stderr(io.StringIO()):
                    code = prover.main()
            finally:
                sys.argv = argv
            self.assertEqual(code, 2)
            receipt = json.loads(receipt_path.read_text())
            self.assertEqual(receipt["status"], "unproven")
            self.assertEqual(receipt["ax"]["reason"], "accessibility_permission_required")
            statuses = {check["id"]: check["status"] for check in receipt["checks"]}
            self.assertEqual(statuses["bundle_identity"], "passed")
            self.assertEqual(statuses["icon_bundle_matches_source"], "passed")
            self.assertEqual(statuses["icon_master_is_1024"], "passed")
            self.assertEqual(statuses["icon_chain_has_1024"], "passed")
            self.assertEqual(statuses["icon_no_placeholder_in_bundle"], "passed")
            for check_id in ("ax_menu_bar", "ax_app_menu_items", "ax_edit_menu_items",
                             "ax_window_menu_items", "ax_window_title"):
                self.assertEqual(statuses[check_id], "unproven", check_id)
                entry = next(check for check in receipt["checks"] if check["id"] == check_id)
                self.assertEqual(entry["reason"], "accessibility_permission_required")

    def test_a_bundle_with_an_unknown_identifier_is_refused(self):
        with tempfile.TemporaryDirectory() as root:
            root = Path(root)
            bundle = self.bundle(root)
            plist = bundle / "Contents/Info.plist"
            plist.write_text(plist.read_text().replace(
                "com.donaldfilimon.abbey", "com.example.somebodyelse"))
            receipt_path = root / "receipt.json"
            argv = sys.argv
            sys.argv = ["prove_menu_icon_macos.py", "--app", str(bundle)]
            try:
                with patch.object(prover, "RECEIPT", receipt_path), \
                        patch.object(prover, "compile_driver", lambda receipt: None), \
                        contextlib.redirect_stdout(io.StringIO()), \
                        contextlib.redirect_stderr(io.StringIO()):
                    code = prover.main()
            finally:
                sys.argv = argv
            self.assertEqual(code, 1)
            receipt = json.loads(receipt_path.read_text())
            self.assertEqual(receipt["failure"], "bundle_identifier_unknown")


class HarnessShape(unittest.TestCase):
    """The wrapper and the native driver are part of the proof."""

    def test_the_wrapper_is_a_thin_shell_that_gates_on_the_prover(self):
        wrapper = (SCRIPTS / "prove-menu-icon-macos.sh").read_text()
        self.assertIn("set -eu", wrapper)
        self.assertIn("exec python3 scripts/prove_menu_icon_macos.py", wrapper)

    def test_the_wrapper_requires_an_explicit_bundle_path(self):
        source = prover.__file__
        text = Path(source).read_text()
        self.assertIn("app_path_required", text)
        self.assertNotIn('glob("*.app")', text)

    def test_the_native_driver_reads_the_menu_bar_and_the_window(self):
        driver = (SCRIPTS / "menu_icon-ax.swift").read_text()
        self.assertIn("kAXMenuBarAttribute", driver)
        self.assertIn("kAXWindowsAttribute", driver)
        self.assertIn("AXUIElementCreateApplication", driver)

    def test_the_receipt_path_is_the_agreed_one(self):
        self.assertEqual(prover.RECEIPT,
                         prover.DESKTOP / "target" / "menu-acceptance" / "menu-receipt.json")


if __name__ == "__main__":
    unittest.main()
