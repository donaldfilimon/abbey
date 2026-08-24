from __future__ import annotations

import importlib.util
from pathlib import Path
import stat
import tempfile
import unittest


SCRIPT = Path(__file__).resolve().parents[1] / "check_claims_sync.py"
SPEC = importlib.util.spec_from_file_location("check_claims_sync", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
claims_sync = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(claims_sync)


def fixture_manifest() -> dict[str, object]:
    evidence = {
        "implementation_refs": ["src/example.rs"],
        "automated_test_refs": ["example::tests"],
        "local_live": {"state": "not_required", "reason": "not required"},
        "external_required": {"state": "not_required", "reason": "not required"},
    }
    return {
        "schema_version": 2,
        "claims": [
            {
                "id": "example-current",
                "name": "Example current capability",
                "status": "current",
                "note": "A deterministic example.",
                "instead": None,
                "evidence": evidence,
                "next_action": "Maintain the regression.",
                "blocker_owner": None,
            }
        ],
    }


class ClaimsSyncTests(unittest.TestCase):
    def desktop_sources(self) -> tuple[str, str, str, str]:
        rust = """tauri::generate_handler![
            commands::app_status,
            commands::app_claims,
            commands::app_routes,
        ]"""
        client = 'export const COMMANDS = ["app_status", "app_claims", "app_routes"] as const;'
        surfaces = """export const SURFACES: readonly Surface[] = [
          { id: "doctor", requires: "read_status" },
          { id: "claims", requires: "read_claims" },
          { id: "routes", requires: "read_routes" },
          { id: "chat", requires: null },
        ] as const;"""
        samples = (
            'export const DESKTOP_READ_CAPABILITIES = '
            '["read_status", "read_claims", "read_routes"] as const;'
        )
        return rust, client, surfaces, samples

    def test_desktop_inventory_derives_counts_and_stable_ids(self) -> None:
        inventory = claims_sync.desktop_inventory(*self.desktop_sources())
        self.assertEqual(
            inventory["commands"], ("app_status", "app_claims", "app_routes")
        )
        self.assertEqual(
            inventory["available"],
            (
                ("doctor", "read_status"),
                ("claims", "read_claims"),
                ("routes", "read_routes"),
            ),
        )
        self.assertEqual(inventory["unavailable"], ("chat",))
        rendered = claims_sync.render_desktop_summary(inventory)
        self.assertIn("3 enumerated commands", rendered)
        self.assertIn("3 available views", rendered)
        self.assertIn("1 unavailable views", rendered)
        self.assertIn("`routes` → `read_routes`", rendered)

    def test_desktop_inventory_rejects_cross_language_or_id_drift(self) -> None:
        rust, client, surfaces, samples = self.desktop_sources()
        with self.assertRaisesRegex(ValueError, "command inventory drift"):
            claims_sync.desktop_inventory(
                rust, client.replace(', "app_routes"', ""), surfaces, samples
            )
        with self.assertRaisesRegex(ValueError, "duplicate desktop surface id"):
            claims_sync.desktop_inventory(
                rust, client, surfaces.replace('id: "chat"', 'id: "routes"'), samples
            )
        with self.assertRaisesRegex(ValueError, "undeclared capability"):
            claims_sync.desktop_inventory(
                rust, client, surfaces.replace('"read_routes"', '"read_memory"'), samples
            )

    def test_desktop_summary_region_is_idempotent_and_strict(self) -> None:
        inventory = claims_sync.desktop_inventory(*self.desktop_sources())
        summary = claims_sync.render_desktop_summary(inventory)
        source = "# Desktop\n\n## The complete Tauri command surface\n"
        once = claims_sync.replace_desktop_summary(source, summary)
        twice = claims_sync.replace_desktop_summary(once, summary)
        self.assertEqual(once, twice)
        with self.assertRaisesRegex(ValueError, "incomplete"):
            claims_sync.replace_desktop_summary(
                f"# Desktop\n{claims_sync.DESKTOP_BEGIN}\n", summary
            )

    def test_lifecycle_statuses_are_accepted_and_counted_only_when_present(self) -> None:
        # Schema 2 added the four terminal lifecycle statuses. This tool used
        # to reject anything outside a five-way allowlist, so a claim entering
        # one of them raised instead of generating docs. Empty states are the
        # case that needs a test: nothing else exercises them until the first
        # real failure, which is the worst moment to find the tool breaks.
        # (Unknown statuses are still rejected -- see the test below.)
        manifest = fixture_manifest()
        self.assertNotIn("Superseded", claims_sync.status_counts(manifest))

        superseded = dict(manifest["claims"][0])
        superseded["id"] = "example-superseded"
        superseded["name"] = "Example superseded capability"
        superseded["status"] = "superseded"
        superseded["instead"] = "example-current"
        manifest["claims"].append(superseded)

        claims_sync.normalize_manifest(manifest)
        self.assertIn("1 Superseded", claims_sync.status_counts(manifest))
        self.assertEqual(claims_sync.status_label("superseded"), "Superseded")

    def test_manifest_requires_object_schema_and_unique_ids(self) -> None:
        manifest = fixture_manifest()
        self.assertEqual(claims_sync.normalize_manifest(manifest), manifest)
        duplicate = fixture_manifest()
        duplicate["claims"].append(dict(duplicate["claims"][0]))
        with self.assertRaisesRegex(ValueError, "duplicate claim id"):
            claims_sync.normalize_manifest(duplicate)
        unknown = fixture_manifest()
        unknown["claims"][0]["status"] = "mystery"
        with self.assertRaisesRegex(ValueError, "unknown status"):
            claims_sync.normalize_manifest(unknown)
        invalid = fixture_manifest()
        invalid["claims"][0]["id"] = "example.invalid"
        with self.assertRaisesRegex(ValueError, "invalid id"):
            claims_sync.normalize_manifest(invalid)

    def test_generated_region_is_semantic_and_idempotent(self) -> None:
        manifest = fixture_manifest()
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "README.md"
            source = "# Example\n\nBody.\n"
            region = claims_sync.render_region(
                "table", manifest, "abc", path, "1 goal (1 done) · 1 checked / 0 open todos"
            )
            once = claims_sync.replace_generated_region(source, "table", region)
            twice = claims_sync.replace_generated_region(once, "table", region)
            self.assertEqual(once, twice)
            self.assertIn("`example-current`", once)
            self.assertIn("Example current capability", once)

    def test_tampered_generated_row_is_repaired(self) -> None:
        manifest = fixture_manifest()
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "README.md"
            region = claims_sync.render_region(
                "table", manifest, "abc", path, "1 goal (1 done) · 1 checked / 0 open todos"
            )
            expected = claims_sync.replace_generated_region("# Example\n", "table", region)
            tampered = expected.replace("Current", "Proposed")
            repaired = claims_sync.replace_generated_region(tampered, "table", region)
            self.assertEqual(repaired, expected)

    def test_duplicate_and_partial_markers_fail_closed(self) -> None:
        manifest = fixture_manifest()
        path = Path("README.md")
        region = claims_sync.render_region(
            "summary", manifest, "abc", path, "1 goal (1 done) · 1 checked / 0 open todos"
        )
        begin = "<!-- BEGIN abbey-generated:claims-summary -->"
        with self.assertRaisesRegex(ValueError, "incomplete"):
            claims_sync.replace_generated_region(begin, "summary", region)
        duplicate = f"{region}\n{region}\n"
        with self.assertRaisesRegex(ValueError, "duplicate"):
            claims_sync.replace_generated_region(duplicate, "summary", region)
        unknown = "<!-- BEGIN abbey-generated:claims-table -->\n"
        with self.assertRaisesRegex(ValueError, "unknown"):
            claims_sync.replace_generated_region(unknown, "summary", region)
        orphan_end = "<!-- END abbey-generated:claims-table -->\n"
        with self.assertRaisesRegex(ValueError, "unknown"):
            claims_sync.replace_generated_region(orphan_end, "summary", region)

    def test_evidence_document_contains_all_evidence_classes(self) -> None:
        rendered = claims_sync.render_evidence_document(
            fixture_manifest(), "abc", "1 goal (1 done) · 1 checked / 0 open todos"
        )
        self.assertIn("Implementation evidence", rendered)
        self.assertIn("Automated tests", rendered)
        self.assertIn("Local/live evidence", rendered)
        self.assertIn("External evidence required", rendered)
        self.assertIn("Blocker owner", rendered)

    def test_atomic_write_preserves_existing_mode_and_defaults_new_files(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            existing = Path(directory) / "existing.md"
            existing.write_text("old", encoding="utf-8")
            existing.chmod(0o640)
            claims_sync.atomic_write(existing, "new")
            self.assertEqual(stat.S_IMODE(existing.stat().st_mode), 0o640)
            created = Path(directory) / "created.md"
            claims_sync.atomic_write(created, "new")
            self.assertEqual(stat.S_IMODE(created.stat().st_mode), 0o644)

    def test_goal_metadata_is_strict_and_counts_workflow(self) -> None:
        goals = """# Goals
## Active
<!-- abbey-goal
id: active
status: in_progress
implementation-evidence: src/example.rs
automated-test-evidence: example::tests
live-external-evidence: not required
next-action: finish it
-->
Body.
"""
        parsed = claims_sync.parse_goal_metadata(goals)
        self.assertEqual(parsed[0]["id"], "active")
        self.assertEqual(
            claims_sync.workflow_summary(goals, "- [x] done\n- [ ] open\n"),
            "1 goal (1 in_progress) · 1 checked / 1 open todo",
        )
        self.assertEqual(
            claims_sync.workflow_summary(goals, "- [x] done\n- [ ] open\n- [ ] more\n"),
            "1 goal (1 in_progress) · 1 checked / 2 open todos",
        )
        with self.assertRaisesRegex(ValueError, "unknown status"):
            claims_sync.parse_goal_metadata(goals.replace("in_progress", "mystery"))
        with self.assertRaisesRegex(ValueError, "invalid stable id"):
            claims_sync.parse_goal_metadata(goals.replace("id: active", "id: ../not-kebab"))


if __name__ == "__main__":
    unittest.main()
