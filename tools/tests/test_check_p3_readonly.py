import importlib.util
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location(
    "check_p3_readonly", ROOT / "tools" / "check_p3_readonly.py"
)
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class Program3BoundaryTest(unittest.TestCase):
    def test_real_surface_is_read_only_and_content_free(self):
        self.assertEqual([], MODULE.scan_paths(MODULE.SURFACES))

    def test_every_program_three_file_is_guarded(self):
        # The boundary is only as good as the list of surfaces it is applied
        # to. A Program 3 file that is not in SURFACES is a file the
        # boundary does not protect, and the guard is silent about it.
        app_core = ROOT / "src" / "app_core"
        program_three = {
            path
            for path in app_core.rglob("*.rs")
            if path.name == "guild_intelligence.rs"
            or "guild_intelligence" in path.parts
        }
        self.assertTrue(program_three)
        self.assertEqual(program_three, set(MODULE.SURFACES))

    def test_forbidden_surface_is_reported(self):
        with tempfile.TemporaryDirectory() as directory:
            bad = Path(directory) / "bad.rs"
            bad.write_text(
                "use reqwest::Client; use std::fs::File; "
                "use std::process::Command; fn execute_change() {}",
                encoding="utf-8",
            )
            findings = MODULE.scan_paths([bad])
            self.assertTrue(any("network client" in item for item in findings))
            self.assertTrue(any("filesystem access" in item for item in findings))
            self.assertTrue(any("process execution" in item for item in findings))
            self.assertTrue(any("effect operation" in item for item in findings))

    def test_a_brace_imported_filesystem_effect_is_reported(self):
        # `use std::{fs};` hides the `std::fs` path from a path-shaped rule,
        # so the effect is reached as `fs::write(...)`. The boundary must
        # catch the call, not only the import spelling.
        with tempfile.TemporaryDirectory() as directory:
            bad = Path(directory) / "bad.rs"
            bad.write_text(
                'use std::{fs};\nfn write_it() { let _ = fs::write("a", "b"); }',
                encoding="utf-8",
            )
            findings = MODULE.scan_paths([bad])
            self.assertTrue(
                any("filesystem access" in item for item in findings),
                f"a brace-imported filesystem effect was not reported: {findings}",
            )


    def test_live_capture_modules_cannot_import_effect_paths(self):
        # A future live-capture module is Program 3 code too. The checker
        # must find it by path and hold it to the same read-only rules, so an
        # adapter cannot quietly reach a store, a runtime tool, an effect, or
        # private content before its contract is reviewed and authorised.
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            adapter = root / "src/app_core/guild_live_capture/adapter.rs"
            adapter.parent.mkdir(parents=True)
            adapter.write_text(
                "use crate::runtime::RuntimeStore; use crate::daemon::runtime_v3; "
                "fn send_reply() {} fn persist_snapshot() {} "
                "struct S { message_content: u8 }",
                encoding="utf-8",
            )
            surfaces = MODULE.live_capture_surfaces(root)
            self.assertIn(adapter, surfaces)
            findings = MODULE.scan_paths(surfaces)
            for label in (
                "durable store",
                "runtime tool",
                "effect operation",
                "private content",
            ):
                self.assertTrue(
                    any(label in item for item in findings),
                    f"live capture {label} was not reported: {findings}",
                )

    def test_checker_scans_live_capture_surfaces_alongside_program_three(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            capture = root / "src/guild_live_capture.rs"
            capture.parent.mkdir(parents=True)
            capture.write_text("pub fn read_only() {}", encoding="utf-8")
            self.assertIn(capture, MODULE.all_surfaces(root))
        self.assertTrue(set(MODULE.SURFACES) <= set(MODULE.all_surfaces(MODULE.ROOT)))


if __name__ == "__main__":
    unittest.main()
