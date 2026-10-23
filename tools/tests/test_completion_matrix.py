from __future__ import annotations

import copy
import importlib.util
from pathlib import Path
import tempfile
import unittest

SCRIPT = Path(__file__).resolve().parents[1] / "check_completion.py"
SPEC = importlib.util.spec_from_file_location("check_completion", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
module = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(module)


class CompletionMatrix(unittest.TestCase):
    def setUp(self) -> None:
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        (self.root / "tasks").mkdir()
        (self.root / "tasks/todo.md").write_text(
            "# Work\n- [ ] Fix provider selection\n- [ ] Fix provider selection\n",
            encoding="utf-8",
        )
        (self.root / "tasks/goals.md").write_text(
            "<!-- abbey-goal\nid: completion\nstatus: in_progress\n-->\n",
            encoding="utf-8",
        )
        self.matrix = {
            "schema_version": 1,
            "milestones": [{
                "id": "baseline", "title": "Baseline", "depends_on": [],
                "status": "open", "owner": "maintainer",
                "acceptance": ["Root and desktop gates pass"], "evidence": {},
                "required_evidence": list(module.MILESTONE_CHANNELS),
            }],
            "requirements": [{
                "id": "provider", "milestone": "baseline", "owner": "maintainer",
                "status": "open", "acceptance": ["Provider transitions select a valid model"],
                "todos": ["Fix provider selection"], "evidence": {},
                "required_evidence": ["source", "automated"],
            }],
            "goals": {"completion": ["baseline"]},
        }

        self.history()

    def history(self):
        import json
        source = "docs/superpowers/plans/retained.md"
        path = self.root / source
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text("- [ ] Restore recovery\n", encoding="utf-8")
        history = {"schema_version": 1, "boundary": "Mapping only", "sources": [source],
                   "open_occurrences": 1, "entries": [{"id": "occurrence", "source": source,
                    "line": 1, "section": "Recovery", "text": "Restore recovery",
                    "requirement": "provider", "milestone": "baseline", "blocker_owner": "maintainer",
                    "acceptance": ["Provider transitions select a valid model"],
                    "disposition": "revalidate_against_current_source_and_evidence"}]}
        def save():
            (self.root / "tasks/completion-historical.json").write_text(json.dumps(history), encoding="utf-8")
        save()
        return history, path, save

    def test_new_historical_checkbox_must_be_mapped(self):
        _, path, _ = self.history()
        path.write_text("- [ ] Restore recovery\n- [ ] New obligation\n", encoding="utf-8")
        self.assertTrue(any("unmapped historical" in e for e in self.errors()))

    def test_historical_owner_and_acceptance_follow_active_requirement(self):
        history, _, save = self.history()
        history["entries"][0]["blocker_owner"] = "wrong owner"
        history["entries"][0]["acceptance"] = ["No acceptance"]
        save()
        self.assertTrue(any("historical binding" in e for e in self.errors()))

    def test_historical_location_cannot_be_forged_or_duplicated(self):
        history, _, save = self.history()
        history["entries"].append(copy.deepcopy(history["entries"][0]))
        history["open_occurrences"] = 2
        save()
        self.assertTrue(any("duplicate historical" in e for e in self.errors()))

    def test_historical_mapping_keeps_closed_evidence_and_ignores_fences(self):
        _, path, _ = self.history()
        path.write_text("- [x] Restore recovery\n````md\n- [ ] Example only\n```\n- [ ] Still an example\n````\n", encoding="utf-8")
        self.assertEqual(self.errors(), [])

    def test_new_plan_cannot_escape_historical_inventory(self):
        self.history()
        (self.root / "docs/superpowers/plans/new.md").write_text("- [ ] New authority\n", encoding="utf-8")
        self.assertTrue(any("unmapped historical" in e for e in self.errors()))

    def test_historical_mapping_rejects_paths_outside_the_repository(self):
        history, _, save = self.history()
        history["entries"][0]["source"] = "../private.md"
        save()
        self.assertTrue(any("historical source" in e for e in self.errors()))

    def errors(self, matrix=None):
        return module.validate(self.matrix if matrix is None else matrix, self.root)

    def test_duplicate_historical_reports_share_one_requirement(self) -> None:
        self.assertEqual(self.errors(), [])

    def test_new_open_todo_requires_an_explicit_owner_and_acceptance(self) -> None:
        with (self.root / "tasks/todo.md").open("a", encoding="utf-8") as stream:
            stream.write("- [ ] New authority\n")
        self.assertTrue(any("unmapped todo" in e for e in self.errors()))

    def test_checked_history_can_keep_its_mapping(self) -> None:
        (self.root / "tasks/todo.md").write_text(
            "- [x] Fix provider selection\n", encoding="utf-8"
        )
        self.assertEqual(self.errors(), [])

    def test_stale_or_ambiguous_bindings_are_rejected(self) -> None:
        self.matrix["requirements"][0]["todos"].append("Missing old task")
        self.assertTrue(any("stale todo" in e for e in self.errors()))
        self.matrix["requirements"][0]["todos"].pop()
        other = copy.deepcopy(self.matrix["requirements"][0])
        other["id"] = "duplicate-owner"
        self.matrix["requirements"].append(other)
        self.assertTrue(any("multiple requirements" in e for e in self.errors()))

    def test_every_unfinished_goal_is_mapped(self) -> None:
        self.matrix["goals"] = {}
        self.assertTrue(any("unmapped goal" in e for e in self.errors()))

    def test_duplicate_requirement_ids_and_dependency_cycles_are_rejected(self) -> None:
        self.matrix["requirements"].append(copy.deepcopy(self.matrix["requirements"][0]))
        self.assertTrue(any("duplicate requirement" in e for e in self.errors()))
        self.matrix["milestones"][0]["depends_on"] = ["baseline"]
        self.assertTrue(any("dependency cycle" in e for e in self.errors()))

    def test_failed_external_evidence_preserves_successful_local_evidence(self) -> None:
        self.matrix["requirements"][0].update({
            "status": "blocked",
            "evidence": {
                "source": {"status": "passed", "refs": ["source receipt"]},
                "automated": {"status": "passed", "refs": ["test receipt"]},
                "external": {"status": "failed", "refs": ["external receipt"]},
            },
        })
        self.assertEqual(self.errors(), [])

    def test_completed_requirement_needs_real_source_and_test_references(self) -> None:
        self.matrix["requirements"][0]["status"] = "complete"
        self.assertTrue(any("source" in e for e in self.errors()))
        self.matrix["requirements"][0]["evidence"] = {
            key: {"status": "passed", "refs": []} for key in ("source", "automated")
        }
        self.assertTrue(any("reference" in e for e in self.errors()))

    def test_completed_milestone_requires_all_platforms_and_requirements(self) -> None:
        milestone = self.matrix["milestones"][0]
        milestone["status"] = "complete"
        self.assertTrue(any("ubuntu_arm64" in e for e in self.errors()))
        self.assertTrue(any("unfinished requirement" in e for e in self.errors()))

    def test_missing_owner_or_acceptance_is_rejected(self) -> None:
        self.matrix["requirements"][0]["owner"] = ""
        self.matrix["requirements"][0]["acceptance"] = []
        self.assertTrue(any("owner" in e for e in self.errors()))
        self.assertTrue(any("acceptance" in e for e in self.errors()))

    def test_required_external_proof_blocks_completion_without_erasing_local_passes(self) -> None:
        requirement = self.matrix["requirements"][0]
        requirement["status"] = "complete"
        requirement["required_evidence"].append("signing")
        requirement["evidence"] = {
            key: {"status": "passed", "refs": ["local receipt"]}
            for key in ("source", "automated")
        }
        (self.root / "tasks/todo.md").write_text(
            "- [x] Fix provider selection\n", encoding="utf-8"
        )
        for state in ("failed", "blocked", "required", "not_required"):
            requirement["evidence"]["signing"] = {"status": state, "refs": ["signing receipt"]}
            self.assertTrue(any("passed signing" in e for e in self.errors()))
        requirement["evidence"]["signing"]["status"] = "passed"
        self.assertEqual(self.errors(), [])

    def test_required_channels_cannot_omit_native_milestone_proof(self) -> None:
        self.matrix["milestones"][0]["required_evidence"] = ["source", "automated"]
        self.assertTrue(any("required evidence channels" in e for e in self.errors()))

    def test_checked_todo_cannot_hide_an_incomplete_requirement(self) -> None:
        self.matrix["requirements"][0]["status"] = "complete"
        self.assertTrue(any("open todo" in e for e in self.errors()))

    def test_repository_matrix_covers_the_live_ledger(self) -> None:
        root = SCRIPT.parent.parent
        self.assertEqual(module.validate(module.load(root), root), [])


if __name__ == "__main__":
    unittest.main()
