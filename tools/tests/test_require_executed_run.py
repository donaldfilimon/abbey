from __future__ import annotations

import importlib.util
from pathlib import Path
import unittest

SCRIPT = Path(__file__).resolve().parents[1] / "ci" / "require_executed_run.py"
SPEC = importlib.util.spec_from_file_location("require_executed_run", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
module = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(module)


class RunEvidence(unittest.TestCase):
    def test_zero_job_startup_failure_is_not_evidence(self) -> None:
        # The exact shape this repository produced repeatedly on 2026-08-08.
        run = {"conclusion": "startup_failure", "status": "completed"}
        ok, reason = module.run_is_real_evidence(run, [])
        self.assertFalse(ok)
        self.assertIn("zero jobs", reason)

    def test_success_with_no_jobs_is_still_not_evidence(self) -> None:
        # Defensive: a green-looking conclusion with nothing executed must not
        # be able to close the phase.
        ok, reason = module.run_is_real_evidence(
            {"status": "completed", "conclusion": "success"}, []
        )
        self.assertFalse(ok)
        self.assertIn("zero jobs", reason)

    def test_failed_steps_are_execution_evidence_but_not_a_pass(self) -> None:
        run = {"status": "completed", "conclusion": "failure"}
        jobs = [{"name": "gate (Linux ARM64)", "conclusion": "failure", "steps": [
            {"name": "Gate both Abbey feature sets", "conclusion": "failure"}
        ]}]
        ok, reason = module.run_is_real_evidence(run, jobs)
        self.assertFalse(ok)
        self.assertIn("executed", reason)

    def test_executed_and_successful_run_is_evidence(self) -> None:
        run = {"status": "completed", "conclusion": "success"}
        jobs = [{"name": "gate (Linux ARM64)", "conclusion": "success", "steps": [
            {"name": "Gate both Abbey feature sets", "conclusion": "success"}
        ]}]
        ok, reason = module.run_is_real_evidence(run, jobs)
        self.assertTrue(ok, reason)
        self.assertIn("1 job", reason)

    def test_skipped_jobs_do_not_count_as_execution(self) -> None:
        # A runner-availability guard that skips every job looks like a green
        # run but proves nothing about the gate.
        run = {"status": "completed", "conclusion": "success"}
        jobs = [{"name": "gate (macOS ARM64 adjunct)", "conclusion": "skipped", "steps": []}]
        ok, reason = module.run_is_real_evidence(run, jobs)
        self.assertFalse(ok)
        self.assertIn("skipped", reason)

    def test_in_progress_sibling_with_succeeded_job_is_not_evidence(self) -> None:
        # Critical fix: a run with one succeeded job and one still-running job
        # is not evidence. This is the shape of polling a two-job matrix mid-flight.
        run = {"conclusion": None, "status": "in_progress"}
        jobs = [
            {"name": "gate (Linux ARM64)", "conclusion": "success"},
            {"name": "gate (macOS ARM64)", "conclusion": None}
        ]
        ok, reason = module.run_is_real_evidence(run, jobs)
        self.assertFalse(ok)
        self.assertIn("in progress", reason)

    def test_single_in_progress_job_is_not_evidence(self) -> None:
        # Important fix: reason must not mislabel conclusion=None as "skipped".
        # conclusion=None means "hasn't finished", not "skipped by workflow logic".
        run = {"conclusion": None}
        jobs = [{"name": "gate (Linux ARM64)", "conclusion": None}]
        ok, reason = module.run_is_real_evidence(run, jobs)
        self.assertFalse(ok)
        self.assertNotIn("skipped", reason)
        self.assertIn("in progress", reason)

    def test_run_status_in_progress_with_all_jobs_succeeded_is_not_evidence(self) -> None:
        # A run with status="in_progress" is not evidence even if all jobs
        # report success (they may not be the final state).
        run = {"conclusion": None, "status": "in_progress"}
        jobs = [
            {"name": "gate (Linux ARM64)", "conclusion": "success"},
            {"name": "gate (macOS ARM64)", "conclusion": "success"}
        ]
        ok, reason = module.run_is_real_evidence(run, jobs)
        self.assertFalse(ok)
        self.assertIn("in_progress", reason)

    def test_missing_run_status_fails_closed(self) -> None:
        run = {"conclusion": "success"}
        jobs = [{"name": "gate", "conclusion": "success", "steps": []}]
        ok, reason = module.run_is_real_evidence(run, jobs)
        self.assertFalse(ok)
        self.assertIn("status", reason)

    def test_failed_or_cancelled_run_cannot_pass_with_successful_jobs(self) -> None:
        jobs = [{
            "name": "gate",
            "conclusion": "success",
            "steps": [{"name": "check", "conclusion": "success"}],
        }]
        for conclusion in ("failure", "cancelled"):
            ok, reason = module.run_is_real_evidence(
                {"status": "completed", "conclusion": conclusion}, jobs
            )
            self.assertFalse(ok)
            self.assertIn("did not succeed", reason)

    def test_successful_job_requires_successful_executed_steps(self) -> None:
        run = {"status": "completed", "conclusion": "success"}
        invalid_steps = (
            [],
            [{"name": "check", "conclusion": "skipped"}],
            [{"name": "check", "conclusion": "failure"}],
            [{"name": "check", "conclusion": "cancelled"}],
            [{"name": "check", "conclusion": None}],
        )
        for steps in invalid_steps:
            ok, _ = module.run_is_real_evidence(
                run, [{"name": "gate", "conclusion": "success", "steps": steps}]
            )
            self.assertFalse(ok)

    def test_skipped_setup_plus_successful_step_is_evidence(self) -> None:
        run = {"status": "completed", "conclusion": "success"}
        jobs = [{
            "name": "gate",
            "conclusion": "success",
            "steps": [
                {"name": "optional setup", "conclusion": "skipped"},
                {"name": "check", "conclusion": "success"},
            ],
        }]
        ok, reason = module.run_is_real_evidence(run, jobs)
        self.assertTrue(ok, reason)
        self.assertIn("1 successful step", reason)
