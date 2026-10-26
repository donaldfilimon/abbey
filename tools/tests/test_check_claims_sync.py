from __future__ import annotations

import copy
import importlib.util
import os
from pathlib import Path
import shutil
import stat
import sys
import tempfile
import unittest
from unittest import mock


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
        "schema_version": 3,
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

    def test_manifest_status_semantics_fail_closed(self) -> None:
        cases = (
            ("current", lambda claim: claim["evidence"].update({"local_live": {"state": "required", "refs": ["proof"]}}), "cannot carry required"),
            ("partial", lambda claim: claim.update({"status": "partial"}), "missing proof"),
            ("proposed", lambda claim: claim.update({"status": "proposed"}), "shipped evidence"),
            ("blocked", lambda claim: claim.update({"status": "blocked"}), "external evidence"),
            ("out_of_scope", lambda claim: (claim.update({"status": "out_of_scope"}), claim["evidence"].update({"local_live": {"state": "verified", "refs": ["proof"]}})), "cannot carry evidence state"),
        )
        for name, mutate, error in cases:
            with self.subTest(name=name):
                manifest = copy.deepcopy(fixture_manifest())
                mutate(manifest["claims"][0])
                with self.assertRaisesRegex(ValueError, error):
                    claims_sync.normalize_manifest(manifest)

    def test_lifecycle_status_semantics_fail_closed(self) -> None:
        for status, error in (
            ("failed", "verified local evidence"),
            ("expired", "fresh local proof"),
        ):
            with self.subTest(status=status):
                manifest = copy.deepcopy(fixture_manifest())
                manifest["claims"][0]["status"] = status
                if status == "failed":
                    manifest["claims"][0]["evidence"]["local_live"] = {
                        "state": "verified",
                        "refs": ["proof"],
                    }
                with self.assertRaisesRegex(ValueError, error):
                    claims_sync.normalize_manifest(manifest)

        revoked = copy.deepcopy(fixture_manifest())
        revoked["claims"][0]["status"] = "revoked"
        revoked["claims"][0]["evidence"]["local_live"] = {
            "state": "required",
            "refs": ["proof"],
        }
        with self.assertRaisesRegex(ValueError, "evidence outstanding"):
            claims_sync.normalize_manifest(revoked)

    def test_superseded_claim_requires_an_existing_replacement(self) -> None:
        manifest = copy.deepcopy(fixture_manifest())
        replacement = copy.deepcopy(manifest["claims"][0])
        replacement["id"] = "example-replacement"
        replacement["name"] = "Example replacement"
        manifest["claims"].append(replacement)
        manifest["claims"][0]["status"] = "superseded"
        manifest["claims"][0]["instead"] = "example-replacement"
        claims_sync.normalize_manifest(manifest)

        for instead in (None, "missing-claim", "example-current"):
            with self.subTest(instead=instead):
                invalid = copy.deepcopy(manifest)
                invalid["claims"][0]["instead"] = instead
                with self.assertRaisesRegex(ValueError, "replacement|invalid instead"):
                    claims_sync.normalize_manifest(invalid)

    def test_old_manifest_schema_is_rejected_before_new_channel_states(self) -> None:
        manifest = fixture_manifest()
        manifest["schema_version"] = 2
        with self.assertRaisesRegex(ValueError, "schema_version must be 3"):
            claims_sync.normalize_manifest(manifest)

    def test_external_channel_failure_preserves_verified_local_proof(self) -> None:
        for state in ("failed", "expired"):
            with self.subTest(state=state):
                manifest = fixture_manifest()
                claim = manifest["claims"][0]
                claim["status"] = state
                claim["evidence"]["local_live"] = {"state": "verified", "refs": ["local.log"]}
                claim["evidence"]["external_required"] = {"state": state, "refs": ["external.log"]}
                claims_sync.normalize_manifest(manifest)
                self.assertEqual(claim["evidence"]["local_live"]["state"], "verified")

    def test_current_cannot_hide_failed_or_expired_channels(self) -> None:
        for state in ("failed", "expired"):
            manifest = fixture_manifest()
            claim = manifest["claims"][0]
            claim["evidence"]["external_required"] = {"state": state, "refs": ["external.log"]}
            with self.assertRaisesRegex(ValueError, "cannot carry"):
                claims_sync.normalize_manifest(manifest)
            claim["status"] = "partial"
            claims_sync.normalize_manifest(manifest)

    def test_evidence_state_shapes_are_exact(self) -> None:
        invalid_states = (
            {"state": "required"},
            {"state": "not_required"},
            {"state": "required", "refs": ["proof"], "reason": "mixed"},
            {"state": "verified", "refs": ["proof"], "extra": True},
        )
        for state in invalid_states:
            with self.subTest(state=state):
                manifest = copy.deepcopy(fixture_manifest())
                manifest["claims"][0]["evidence"]["local_live"] = state
                with self.assertRaisesRegex(ValueError, "local_live"):
                    claims_sync.normalize_manifest(manifest)

    def test_nonblocked_goal_cannot_carry_blocker_owner(self) -> None:
        goals = """## Active
<!-- abbey-goal
id: active
status: in_progress
implementation-evidence: source
automated-test-evidence: tests
live-external-evidence: local
next-action: continue
blocker-owner: repository owner
-->
"""
        with self.assertRaisesRegex(ValueError, "nonblocked goal"):
            claims_sync.parse_goal_metadata(goals)

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

    def _test_ref_root(self, directory: str) -> Path:
        root = Path(directory)
        (root / "src").mkdir()
        (root / "tests").mkdir()
        (root / "desktop/src-tauri/src").mkdir(parents=True)
        (root / "tools/tests").mkdir(parents=True)
        (root / "src/example.rs").write_text(
            "mod tests {\n    fn present_test_is_found() {}\n}\n",
            encoding="utf-8",
        )
        (root / "tests/cli_surface.rs").write_text(
            "fn top_level_integration_test() {}\n", encoding="utf-8"
        )
        (root / "desktop/src-tauri/src/v3.rs").write_text(
            "mod tests {\n    fn desktop_backend_test() {}\n}\n",
            encoding="utf-8",
        )
        (root / "tools/tests/test_thing.py").write_text(
            "def test_python_thing():\n    pass\n", encoding="utf-8"
        )
        (root / "prove.sh").write_text("#!/bin/sh\n", encoding="utf-8")
        return root

    def test_test_reference_with_present_name_passes(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = self._test_ref_root(directory)
            claim = fixture_manifest()["claims"][0]
            claim["evidence"]["automated_test_refs"] = [
                "src/example.rs::tests::present_test_is_found",
                "tests/cli_surface.rs::top_level_integration_test",
                "desktop/src-tauri/src/v3.rs::tests::desktop_backend_test",
                "tools/tests/test_thing.py::test_python_thing",
                "prove.sh",
            ]
            self.assertEqual(
                claims_sync.validate_test_references([claim], root=root), []
            )

    def test_test_reference_native_workspace_declared_and_missing_names(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = self._test_ref_root(directory)
            sources = {
                "crates/abbey-learning/src/native/tests.rs": "native_tensor_update_test",
                "crates/abbey-learning/tests/pipeline.rs": "native_pipeline_roundtrip_test",
            }
            for relative, name in sources.items():
                path = root / relative
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(f"#[test]\nfn {name}() {{}}\n", encoding="utf-8")
            claim = fixture_manifest()["claims"][0]
            claim["evidence"]["automated_test_refs"] = [
                f"{relative}::{name}" for relative, name in sources.items()
            ]
            self.assertEqual(
                claims_sync.validate_test_references([claim], root=root), []
            )
            claim["evidence"]["automated_test_refs"].append(
                "crates/abbey-learning/tests/pipeline.rs::missing_native_test_function"
            )
            problems = claims_sync.validate_test_references([claim], root=root)
            self.assertEqual(len(problems), 1)
            self.assertIn("missing_native_test_function", problems[0])
            self.assertIn("declared test", problems[0])

    def test_test_reference_with_missing_name_fails(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = self._test_ref_root(directory)
            claim = fixture_manifest()["claims"][0]
            claim["evidence"]["automated_test_refs"] = [
                "src/example.rs::tests::this_test_was_renamed"
            ]
            problems = claims_sync.validate_test_references([claim], root=root)
            self.assertEqual(len(problems), 1)
            self.assertIn("example-current", problems[0])
            self.assertIn("this_test_was_renamed", problems[0])

    def test_test_reference_with_missing_file_fails(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = self._test_ref_root(directory)
            claim = fixture_manifest()["claims"][0]
            claim["evidence"]["automated_test_refs"] = ["src/does_not_exist.rs"]
            problems = claims_sync.validate_test_references([claim], root=root)
            self.assertEqual(len(problems), 1)
            self.assertIn("missing file", problems[0])

    def test_test_reference_prose_with_whitespace_is_left_unresolved(self) -> None:
        # "workflow job steps in .github/workflows/rust.yml" is real registry
        # evidence: narrative text mentioning a filename, not a checkable
        # path or test ref. A ref containing whitespace is never a real path
        # or `path::name` ref, so it is deliberately skipped rather than
        # reported as a missing file.
        with tempfile.TemporaryDirectory() as directory:
            root = self._test_ref_root(directory)
            claim = fixture_manifest()["claims"][0]
            claim["evidence"]["automated_test_refs"] = [
                "workflow job steps in .github/workflows/rust.yml"
            ]
            self.assertEqual(
                claims_sync.validate_test_references([claim], root=root), []
            )


class ManifestInvocationTests(unittest.TestCase):
    """The manifest step shells out to a real `cargo run`.

    Two failure modes used to reach the gate as opaque tracebacks: a budget
    that expired while the crate was still compiling, and a build error whose
    real diagnosis lived on cargo's stderr but was never shown.
    """

    def test_timeout_budget_defaults_high_and_is_overridable(self) -> None:
        original = os.environ.pop(claims_sync.TIMEOUT_ENV, None)
        try:
            self.assertEqual(
                claims_sync.manifest_timeout_seconds(),
                claims_sync.DEFAULT_MANIFEST_TIMEOUT_SECONDS,
            )
            # A two-minute budget was shorter than a normal rebuild of the
            # crate, so it turned an ordinary rebuild into a gate failure.
            self.assertGreater(claims_sync.DEFAULT_MANIFEST_TIMEOUT_SECONDS, 120)
            os.environ[claims_sync.TIMEOUT_ENV] = "1800"
            self.assertEqual(claims_sync.manifest_timeout_seconds(), 1800)
        finally:
            os.environ.pop(claims_sync.TIMEOUT_ENV, None)
            if original is not None:
                os.environ[claims_sync.TIMEOUT_ENV] = original

    def test_invalid_timeout_budget_is_refused(self) -> None:
        original = os.environ.get(claims_sync.TIMEOUT_ENV)
        try:
            for value in ("soon", "0", "-1"):
                with self.subTest(value=value):
                    os.environ[claims_sync.TIMEOUT_ENV] = value
                    with self.assertRaises(SystemExit) as raised:
                        claims_sync.manifest_timeout_seconds()
                    self.assertIn(claims_sync.TIMEOUT_ENV, str(raised.exception))
        finally:
            if original is None:
                os.environ.pop(claims_sync.TIMEOUT_ENV, None)
            else:
                os.environ[claims_sync.TIMEOUT_ENV] = original

    def _run_with_stubbed_cargo(self, outcome: object) -> None:
        """Drive `canonical_manifest` with a stubbed cargo invocation.

        The seam is the module's own `run_manifest_command`, so the test never
        mutates the shared standard-library `subprocess` module.
        """
        original_runner = claims_sync.run_manifest_command
        original_cargo = claims_sync.find_cargo
        original_policy = claims_sync.assert_qualified_toolchain

        def fake_runner(*args: object, **kwargs: object) -> tuple[str, str, int]:
            if isinstance(outcome, BaseException):
                raise outcome
            return (outcome.stdout, outcome.stderr, outcome.returncode)

        claims_sync.find_cargo = lambda: "/bin/true"
        claims_sync.assert_qualified_toolchain = lambda: None
        claims_sync.run_manifest_command = fake_runner
        try:
            claims_sync.canonical_manifest()
        finally:
            claims_sync.run_manifest_command = original_runner
            claims_sync.find_cargo = original_cargo
            claims_sync.assert_qualified_toolchain = original_policy

    def test_expired_build_budget_reports_the_budget_not_a_traceback(self) -> None:
        # A real invocation: the budget expires while the child is still
        # running, and the step has to say so instead of raising a traceback.
        with self.assertRaises(SystemExit) as raised:
            claims_sync.run_manifest_command(
                [sys.executable, "-c", "import time; time.sleep(30)"],
                os.environ.copy(),
                1,
            )
        message = str(raised.exception)
        self.assertIn(claims_sync.TIMEOUT_ENV, message)
        self.assertIn("building", message)
        self.assertNotIn("Traceback", message)

    @unittest.skipUnless(os.name == "posix", "POSIX pipe cleanup contract")
    def test_expired_build_budget_closes_pipe_readers(self) -> None:
        # Capture a real scratch child, never the manifest or Cargo. Retain it
        # so garbage collection cannot hide unclosed readers from this check.
        children = []
        real_popen = claims_sync.subprocess.Popen

        def capture(*args, **kwargs):
            child = real_popen(*args, **kwargs)
            children.append(child)
            return child

        try:
            with mock.patch.object(claims_sync.subprocess, "Popen", side_effect=capture):
                with self.assertRaises(SystemExit):
                    claims_sync.run_manifest_command(
                        [sys.executable, "-c", "import time; time.sleep(30)"],
                        os.environ.copy(),
                        1,
                    )
            self.assertEqual(len(children), 1)
            child = children[0]
            self.assertIsNotNone(child.returncode)
            self.assertTrue(child.stdout.closed)
            self.assertTrue(child.stderr.closed)
        finally:
            for child in children:
                child.stdout.close()
                child.stderr.close()

    def test_failed_build_reports_cargos_own_diagnosis(self) -> None:
        with self.assertRaises(SystemExit) as raised:
            self._run_with_stubbed_cargo(
                _Result(
                    stdout="",
                    stderr="error[E0425]: cannot find value `x` in this scope",
                    returncode=101,
                )
            )
        message = str(raised.exception)
        self.assertIn("cannot find value", message)
        self.assertNotIn("Traceback", message)

    def test_non_json_manifest_output_is_refused_clearly(self) -> None:
        with self.assertRaises(SystemExit) as raised:
            self._run_with_stubbed_cargo(_Result(stdout="warning: something else\n"))
        self.assertIn("not valid JSON", str(raised.exception))

    def _capture_the_manifest_command(self) -> dict[str, object]:
        captured: dict[str, object] = {}

        def fake_runner(command: list[str], env: dict[str, str], timeout: int):
            captured["command"] = command
            captured["path"] = env.get("PATH", "")
            return ("not json", "", 0)

        original_runner = claims_sync.run_manifest_command
        original_cargo = claims_sync.find_cargo
        original_policy = claims_sync.assert_qualified_toolchain
        claims_sync.run_manifest_command = fake_runner
        claims_sync.find_cargo = lambda: "/toolchain/bin/cargo"
        claims_sync.assert_qualified_toolchain = lambda: None
        try:
            with self.assertRaises(SystemExit):
                claims_sync.canonical_manifest()
        finally:
            claims_sync.run_manifest_command = original_runner
            claims_sync.find_cargo = original_cargo
            claims_sync.assert_qualified_toolchain = original_policy
        return captured

    def test_the_manifest_build_cannot_rewrite_the_tracked_lockfile(self) -> None:
        # A resolved-graph difference must not silently change Cargo.lock.
        command = self._capture_the_manifest_command()["command"]
        self.assertIn("--locked", command)

    def test_the_manifest_build_prefers_the_cargo_s_own_toolchain(self) -> None:
        # The cargo's own directory has to come first, or an earlier rustc
        # would shadow the one it was built with.
        path = str(self._capture_the_manifest_command()["path"])
        self.assertTrue(path.startswith("/toolchain/bin"), path)

    def test_manifest_prefers_the_pinned_toolchain_over_the_path_cargo(self) -> None:
        # A non-rustup cargo on PATH (a Homebrew install, say) does not read
        # `rust-toolchain.toml`, so the manifest would come from an
        # unqualified compiler. The pinned toolchain wins when rustup can
        # resolve it.
        original_pinned = claims_sync.pinned_toolchain_cargo
        try:
            # A candidate has to be a real executable, so the pinned one is
            # the running interpreter here.
            claims_sync.pinned_toolchain_cargo = lambda: sys.executable
            with mock.patch.object(
                claims_sync, "cargo_builds_the_pinned_toolchain", return_value=True
            ):
                self.assertEqual(claims_sync.find_cargo(), sys.executable)
        finally:
            claims_sync.pinned_toolchain_cargo = original_pinned

    def test_manifest_falls_back_to_the_path_cargo_without_a_pinned_toolchain(self) -> None:
        original_pinned = claims_sync.pinned_toolchain_cargo
        original_which = shutil.which
        try:
            claims_sync.pinned_toolchain_cargo = lambda: None
            shutil.which = lambda name: sys.executable if name == "cargo" else None
            with mock.patch.object(
                claims_sync, "cargo_builds_the_pinned_toolchain", return_value=True
            ):
                self.assertEqual(claims_sync.find_cargo(), sys.executable)
        finally:
            claims_sync.pinned_toolchain_cargo = original_pinned
            shutil.which = original_which

    def test_manifest_refuses_a_cargo_that_would_not_build_the_pinned_toolchain(self) -> None:
        # Falling back to an arbitrary compiler is the failure this closes:
        # the claims digest is the repository's canonical evidence anchor.
        original_pinned = claims_sync.pinned_toolchain_cargo
        original_which = shutil.which
        try:
            claims_sync.pinned_toolchain_cargo = lambda: None
            shutil.which = lambda name: sys.executable if name == "cargo" else None
            with mock.patch.object(
                claims_sync, "cargo_builds_the_pinned_toolchain", return_value=False
            ):
                with self.assertRaises(SystemExit) as raised:
                    claims_sync.find_cargo()
            self.assertIn("pinned toolchain", str(raised.exception))
        finally:
            claims_sync.pinned_toolchain_cargo = original_pinned
            shutil.which = original_which

    def test_pinned_toolchain_cargo_survives_a_missing_or_failing_rustup(self) -> None:
        original_which = shutil.which
        original_run = claims_sync.subprocess.run
        try:
            # rustup is absent.
            shutil.which = lambda name: None
            self.assertIsNone(claims_sync.pinned_toolchain_cargo())

            # rustup is present but cannot resolve the toolchain.
            shutil.which = lambda name: "/usr/bin/rustup" if name == "rustup" else None
            claims_sync.subprocess.run = lambda *args, **kwargs: _Result(
                stderr="boom", returncode=1
            )
            self.assertIsNone(claims_sync.pinned_toolchain_cargo())
        finally:
            shutil.which = original_which
            claims_sync.subprocess.run = original_run

    def test_pinned_toolchain_cargo_ignores_an_environment_toolchain_override(self) -> None:
        # `RUSTUP_TOOLCHAIN` overrides the directory override, so the pinned
        # toolchain would not be the one that gets resolved.
        captured: dict[str, object] = {}

        def fake_run(command, cwd=None, capture_output=None, text=None, timeout=None, env=None):
            captured["env"] = env
            return _Result(stdout="/toolchain/bin/cargo\n")

        original_which = shutil.which
        original_run = claims_sync.subprocess.run
        original_env = os.environ.get("RUSTUP_TOOLCHAIN")
        try:
            shutil.which = lambda name: "/usr/bin/rustup" if name == "rustup" else None
            claims_sync.subprocess.run = fake_run
            os.environ["RUSTUP_TOOLCHAIN"] = "stable"
            claims_sync.pinned_toolchain_cargo()
            self.assertNotIn("RUSTUP_TOOLCHAIN", captured["env"])
        finally:
            shutil.which = original_which
            claims_sync.subprocess.run = original_run
            if original_env is None:
                os.environ.pop("RUSTUP_TOOLCHAIN", None)
            else:
                os.environ["RUSTUP_TOOLCHAIN"] = original_env

    def test_substituted_compiler_fails_the_manifest_closed(self) -> None:
        # The claims digest is the repository's canonical evidence anchor, so
        # it must come from the qualified toolchain. The repository's own
        # policy tool decides that; the manifest step has to honour it.
        original_run = claims_sync.subprocess.run
        try:
            claims_sync.subprocess.run = lambda *args, **kwargs: _Result(
                stderr="stable qualification rejects compiler substitution: environment RUSTC",
                returncode=1,
            )
            with self.assertRaises(SystemExit) as raised:
                claims_sync.assert_qualified_toolchain()
            self.assertIn("qualified toolchain", str(raised.exception))
            self.assertIn("rejects compiler substitution", str(raised.exception))
        finally:
            claims_sync.subprocess.run = original_run


class _Result:
    """A stand-in for a completed subprocess."""

    def __init__(self, stdout: str = "", stderr: str = "", returncode: int = 0) -> None:
        self.stdout = stdout
        self.stderr = stderr
        self.returncode = returncode


if __name__ == "__main__":
    unittest.main()
