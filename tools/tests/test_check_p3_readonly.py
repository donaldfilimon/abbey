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


if __name__ == "__main__":
    unittest.main()
