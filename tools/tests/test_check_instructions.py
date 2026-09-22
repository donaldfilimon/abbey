import importlib.util
import shutil
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location(
    "check_instructions", ROOT / "tools" / "check_instructions.py"
)
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class InstructionDriftTest(unittest.TestCase):
    def setUp(self):
        self._dir = tempfile.TemporaryDirectory()
        self.root = Path(self._dir.name)
        for name in ("AGENTS.md", "CLAUDE.md", "check.sh", "install.sh"):
            shutil.copy(ROOT / name, self.root / name)
        shutil.copytree(ROOT / "tools", self.root / "tools", ignore=shutil.ignore_patterns("__pycache__"))
        shutil.copytree(ROOT / "src", self.root / "src")

    def tearDown(self):
        self._dir.cleanup()

    def edit(self, name, old, new):
        path = self.root / name
        text = path.read_text(encoding="utf-8")
        self.assertIn(old, text)
        path.write_text(text.replace(old, new, 1), encoding="utf-8")

    def test_real_tree_is_consistent(self):
        self.assertEqual([], MODULE.check(ROOT))
        self.assertEqual([], MODULE.check(self.root))

    def test_new_heading_in_claude_md_is_reported(self):
        self.edit("CLAUDE.md", "## Commands\n", "## Conventions\n\n- a rule\n\n## Commands\n")
        self.assertTrue(any("headings" in item for item in MODULE.check(self.root)))

    def test_extra_prose_in_claude_md_is_reported(self):
        self.edit("CLAUDE.md", "\n## Commands\n", "\nAlso: never do X.\n\n## Commands\n")
        self.assertTrue(any("pointer paragraph" in item for item in MODULE.check(self.root)))

    def test_prose_after_the_module_table_is_reported(self):
        self.edit("CLAUDE.md", "\n<!-- machine-git-policy -->", "\nA stray rule.\n\n<!-- machine-git-policy -->")
        self.assertTrue(any("only the module table" in item for item in MODULE.check(self.root)))

    def test_git_policy_drift_is_reported(self):
        self.edit("CLAUDE.md", "Work on the default branch", "Work on any branch")
        self.assertTrue(any("git-policy block differs" in item for item in MODULE.check(self.root)))

    def test_missing_module_is_reported(self):
        (self.root / "src" / "mesh.rs").unlink()
        self.assertTrue(any("src/mesh.rs" in item for item in MODULE.check(self.root)))

    def test_missing_command_script_is_reported(self):
        (self.root / "tools" / "check_p3_readonly.py").unlink()
        self.assertTrue(any("tools/check_p3_readonly.py" in item for item in MODULE.check(self.root)))

    def test_non_pointer_editor_surface_is_reported(self):
        (self.root / "GEMINI.md").write_text("# Rules\n\nNever run the gate.\n", encoding="utf-8")
        self.assertTrue(any("GEMINI.md" in item for item in MODULE.check(self.root)))

    def test_pointer_editor_surface_is_accepted(self):
        (self.root / "GEMINI.md").write_text("# GEMINI.md\n\nSee AGENTS.md — canonical.\n", encoding="utf-8")
        self.assertEqual([], MODULE.check(self.root))


if __name__ == "__main__":
    unittest.main()
