#!/usr/bin/env python3
"""Decide whether an Actions run is execution evidence.

Phase 2's bar: "close the CI TODOs only after a run with actual jobs +
successful steps; a zero-job `startup_failure` is infrastructure evidence,
never source-test evidence". This module makes that rule executable so the
distinction cannot be lost in a summary.
"""

from __future__ import annotations

import json
import subprocess
import sys


def run_is_real_evidence(run: dict, jobs: list[dict]) -> tuple[bool, str]:
    """Return (is_evidence, reason) for one workflow run.

    Evidence requires jobs that actually ran and succeeded. A run with no jobs,
    in-progress jobs, or incomplete status never reached completion.
    """
    # Check if run is still in progress based on status field
    status = run.get("status")
    if status != "completed":
        return False, f"run still in progress: status is {status!r}"
    # Check if no jobs were scheduled
    if not jobs:
        return False, (
            f"zero jobs scheduled (run conclusion={run.get('conclusion')!r}); "
            "infrastructure evidence only, not source-test evidence"
        )

    # Check for in-progress jobs (conclusion=None means not yet finished)
    in_progress = [j for j in jobs if j.get("conclusion") is None]
    if in_progress:
        names = ", ".join(str(j.get("name")) for j in in_progress)
        return False, f"run still in progress: {len(in_progress)} job(s) not yet finished ({names})"

    # Now check for truly skipped jobs vs executed jobs
    # At this point, no job has conclusion=None, so all are either "skipped" or something else
    executed = [j for j in jobs if j.get("conclusion") != "skipped"]

    # If all jobs are skipped, nothing executed
    if not executed:
        names = ", ".join(str(j.get("name")) for j in jobs)
        return False, f"all {len(jobs)} job(s) skipped ({names}); nothing executed"

    # Check if any executed jobs failed
    failed = [j for j in executed if j.get("conclusion") != "success"]
    if failed:
        names = ", ".join(str(j.get("name")) for j in failed)
        return False, (
            f"{len(executed)} job(s) executed but {len(failed)} failed ({names}); "
            "real execution evidence, not a passing gate"
        )

    successful_steps = 0
    for job in executed:
        steps = job.get("steps")
        if not isinstance(steps, list) or not steps:
            return False, f"successful job {job.get('name')!r} reported no steps"
        unfinished = [step for step in steps if step.get("conclusion") is None]
        if unfinished:
            return False, f"successful job {job.get('name')!r} has unfinished steps"
        ran = [step for step in steps if step.get("conclusion") != "skipped"]
        if not ran:
            return False, f"successful job {job.get('name')!r} executed no steps"
        failed_steps = [step for step in ran if step.get("conclusion") != "success"]
        if failed_steps:
            return False, f"successful job {job.get('name')!r} contains failed steps"
        successful_steps += len(ran)

    if run.get("conclusion") != "success":
        return False, f"completed run did not succeed: conclusion is {run.get('conclusion')!r}"

    return True, (
        f"{len(executed)} job(s) executed and succeeded with "
        f"{successful_steps} successful step(s)"
    )


def main(argv: list[str]) -> int:
    if len(argv) != 2:
        print("usage: require_executed_run.py <run-id>", file=sys.stderr)
        return 2
    run_id = argv[1]
    repo = "donaldfilimon/abbey"
    run = json.loads(
        subprocess.run(
            ["gh", "api", f"repos/{repo}/actions/runs/{run_id}"],
            capture_output=True, text=True, check=True,
        ).stdout
    )
    jobs = json.loads(
        subprocess.run(
            ["gh", "api", f"repos/{repo}/actions/runs/{run_id}/jobs"],
            capture_output=True, text=True, check=True,
        ).stdout
    ).get("jobs", [])
    ok, reason = run_is_real_evidence(run, jobs)
    print(f"{'EVIDENCE' if ok else 'NOT EVIDENCE'}: {reason}")
    return 0 if ok else 1


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
