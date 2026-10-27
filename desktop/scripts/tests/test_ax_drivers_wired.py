"""The Accessibility surface/control drivers are wired into the desktop gate.

Each driver is opt-in: `desktop/check.sh` runs it only when its acceptance
variable is set. When it does run, it runs *required*: a missing bundle, a
non-macOS host or a bundle of the wrong edition fails the gate instead of
printing "skipped" and passing. Unrequired runs keep the old skip behaviour.

Never opens a window and never compiles the Swift driver: `build_driver` is
patched to raise, so any test that reached it would fail.
"""

import contextlib
import importlib.util
import io
import json
import os
from pathlib import Path
import re
import sys
import tempfile
import unittest
from unittest.mock import patch

SCRIPTS = Path(__file__).resolve().parents[1]
DESKTOP = SCRIPTS.parent
sys.path.insert(0, str(SCRIPTS))

SAFE_IDENTIFIER = "com.donaldfilimon.abbey"

DRIVERS = {
    "surfaces": "drive-surfaces-macos.py",
    "controls": "drive-controls-macos.py",
}


def load_driver(filename):
    """Import a driver whose filename has hyphens."""
    name = "ax_driver_" + filename.replace("-", "_").removesuffix(".py")
    spec = importlib.util.spec_from_file_location(name, SCRIPTS / filename)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def bundle(root, name, identifier):
    """A fixture bundle with a real Info.plist and an executable."""
    path = Path(root) / name
    (path / "Contents/MacOS").mkdir(parents=True)
    (path / "Contents/MacOS/abbey-desktop").write_bytes(b"")
    (path / "Contents/Info.plist").write_text(
        '<?xml version="1.0" encoding="UTF-8"?>\n'
        '<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" '
        '"http://www.apple.com/DTDs/PropertyList-1.0.dtd">\n'
        '<plist version="1.0"><dict>'
        f"<key>CFBundleIdentifier</key><string>{identifier}</string>"
        "</dict></plist>\n"
    )
    return path


def gate_block(variable):
    """The body of `if [ "${VARIABLE:-0}" = "1" ]; then ... fi` in check.sh."""
    text = (DESKTOP / "check.sh").read_text()
    pattern = (
        r'if \[ "\$\{' + re.escape(variable) + r':-0\}" = "1" \]; then\n'
        r"(.*?)\nfi\n"
    )
    match = re.search(pattern, text, re.S)
    return match.group(1) if match else None


def never_build():
    raise AssertionError("build_driver must not run on a refused bundle")


def run_main(module, system, env):
    out = io.StringIO()
    with patch.dict(os.environ, env, clear=False), \
            patch.object(module.platform, "system", return_value=system), \
            patch.object(module, "build_driver", side_effect=never_build), \
            contextlib.redirect_stdout(out):
        code = module.main()
    lines = [line for line in out.getvalue().splitlines() if line.strip()]
    return code, json.loads(lines[-1]) if lines else {}


class GateWiring(unittest.TestCase):
    """check.sh runs each driver, required, when its variable is set."""

    def assert_wired(self, variable, driver):
        block = gate_block(variable)
        self.assertIsNotNone(block, f"check.sh has no {variable} block")
        self.assertIn(f"scripts/{driver}", block)
        self.assertRegex(block, r"ABBEY_DESKTOP_APP=")
        self.assertIn("ABBEY_DESKTOP_DRIVER_REQUIRED=1", block)
        self.assertIn("ABBEY_DESKTOP_EXPECT_EDITION=", block)

    def test_surface_driver_runs_when_its_variable_is_set(self):
        self.assert_wired("ABBEY_DESKTOP_SURFACE_ACCEPTANCE", DRIVERS["surfaces"])

    def test_control_driver_runs_when_its_variable_is_set(self):
        self.assert_wired("ABBEY_DESKTOP_CONTROL_ACCEPTANCE", DRIVERS["controls"])


class RequiredDrivers(unittest.TestCase):
    """A required driver never reports "skipped" as success."""

    def setUp(self):
        self._env = patch.dict(os.environ, {}, clear=False)
        self._env.start()
        for key in ("ABBEY_DESKTOP_BINARY", "ABBEY_DESKTOP_DRIVER_REQUIRED",
                    "ABBEY_DESKTOP_EXPECT_EDITION", "ABBEY_DESKTOP_APP"):
            os.environ.pop(key, None)
        self.addCleanup(self._env.stop)

    def test_a_required_driver_fails_instead_of_skipping_without_a_bundle(self):
        with tempfile.TemporaryDirectory() as root:
            missing = str(Path(root) / "missing" / "Abbey.app")
            for kind, filename in DRIVERS.items():
                module = load_driver(filename)
                for system in ("Darwin", "Linux"):
                    with self.subTest(driver=kind, system=system):
                        code, report = run_main(module, system, {
                            "ABBEY_DESKTOP_DRIVER_REQUIRED": "1",
                            "ABBEY_DESKTOP_APP": missing,
                            "ABBEY_DESKTOP_EXPECT_EDITION": "safe",
                        })
                        self.assertNotEqual(code, 0)
                        self.assertEqual(report.get("status"), "failed")

    def test_a_required_surface_driver_does_not_accept_a_bare_binary(self):
        with tempfile.TemporaryDirectory() as root:
            binary = Path(root) / "abbey-desktop"
            binary.write_bytes(b"")
            module = load_driver(DRIVERS["surfaces"])
            code, report = run_main(module, "Darwin", {
                "ABBEY_DESKTOP_DRIVER_REQUIRED": "1",
                "ABBEY_DESKTOP_APP": str(Path(root) / "missing.app"),
                "ABBEY_DESKTOP_BINARY": str(binary),
                "ABBEY_DESKTOP_EXPECT_EDITION": "safe",
            })
            self.assertNotEqual(code, 0)
            self.assertEqual(report.get("status"), "failed")

    def test_a_required_driver_refuses_the_wrong_edition_bundle(self):
        with tempfile.TemporaryDirectory() as root:
            app = bundle(root, "Abbey.app", SAFE_IDENTIFIER)
            for kind, filename in DRIVERS.items():
                module = load_driver(filename)
                with self.subTest(driver=kind):
                    code, report = run_main(module, "Darwin", {
                        "ABBEY_DESKTOP_DRIVER_REQUIRED": "1",
                        "ABBEY_DESKTOP_APP": str(app),
                        "ABBEY_DESKTOP_EXPECT_EDITION": "personal",
                    })
                    self.assertNotEqual(code, 0)
                    self.assertEqual(report.get("status"), "failed")
                    self.assertEqual(report.get("reason"), "edition_mismatch")
                    self.assertEqual(report.get("found"), "safe")
                    self.assertEqual(report.get("expected"), "personal")

    def test_a_required_driver_drives_the_expected_edition_bundle(self):
        with tempfile.TemporaryDirectory() as root:
            app = bundle(root, "Abbey.app", SAFE_IDENTIFIER)
            for kind, filename in DRIVERS.items():
                module = load_driver(filename)
                with self.subTest(driver=kind):
                    # Reaching build_driver is the proof the bundle passed.
                    with self.assertRaisesRegex(AssertionError, "build_driver"):
                        run_main(module, "Darwin", {
                            "ABBEY_DESKTOP_DRIVER_REQUIRED": "1",
                            "ABBEY_DESKTOP_APP": str(app),
                            "ABBEY_DESKTOP_EXPECT_EDITION": "safe",
                        })

    def test_a_required_driver_refuses_an_unknown_identifier(self):
        with tempfile.TemporaryDirectory() as root:
            app = bundle(root, "Abbey.app", "com.example.not-abbey")
            for kind, filename in DRIVERS.items():
                module = load_driver(filename)
                with self.subTest(driver=kind):
                    code, report = run_main(module, "Darwin", {
                        "ABBEY_DESKTOP_DRIVER_REQUIRED": "1",
                        "ABBEY_DESKTOP_APP": str(app),
                        "ABBEY_DESKTOP_EXPECT_EDITION": "safe",
                    })
                    self.assertNotEqual(code, 0)
                    self.assertEqual(report.get("reason"), "edition_mismatch")

    def test_a_required_driver_refuses_an_unknown_expected_edition(self):
        with tempfile.TemporaryDirectory() as root:
            app = bundle(root, "Abbey.app", SAFE_IDENTIFIER)
            for kind, filename in DRIVERS.items():
                module = load_driver(filename)
                with self.subTest(driver=kind):
                    code, report = run_main(module, "Darwin", {
                        "ABBEY_DESKTOP_DRIVER_REQUIRED": "1",
                        "ABBEY_DESKTOP_APP": str(app),
                        "ABBEY_DESKTOP_EXPECT_EDITION": "",
                    })
                    self.assertNotEqual(code, 0)
                    self.assertEqual(report.get("status"), "failed")

    def test_an_unrequired_driver_still_skips(self):
        with tempfile.TemporaryDirectory() as root:
            missing = str(Path(root) / "missing" / "Abbey.app")
            for kind, filename in DRIVERS.items():
                module = load_driver(filename)
                for system in ("Darwin", "Linux"):
                    with self.subTest(driver=kind, system=system):
                        code, report = run_main(module, system, {
                            "ABBEY_DESKTOP_APP": missing,
                        })
                        self.assertEqual(code, 0)
                        self.assertEqual(report.get("status"), "skipped")


if __name__ == "__main__":
    unittest.main()
