#!/usr/bin/env python3
"""Decide whether an Actions run is execution evidence.

The evidence bar is an exact-head completed run with an actually executed,
successful qualifying gate and all of that gate's required steps. Linux ARM64,
macOS ARM64, and the isolated hosted-fork gate are distinguishable narrower
facts; none is silently promoted to cross-platform proof. A zero-job
`startup_failure` is infrastructure evidence, never source-test evidence.
"""

from __future__ import annotations

import json
import subprocess
import sys

RUST_WORKFLOW_PATH = ".github/workflows/rust.yml"
PRIMARY_JOB = "gate (Linux ARM64)"
REQUIRED_PRIMARY_STEPS = {
    "Check out Abbey",
    "Check out the verified ABI dependency",
    "Check out the public WDBX substrate",
    "Install pinned ABI toolchain",
    "Install pinned Abbey toolchain",
    "Build the real ABI binary",
    "Gate both Abbey feature sets",
    "Install cargo-audit if absent",
    "RustSec dependency scan",
    "Warning-denied API docs and release binary",
    "Release install and provider inventory smoke",
    "ABI-backed local mesh proof",
    "Clean runner workspace",
}
MACOS_JOB = "gate (macOS ARM64 adjunct)"
HOSTED_FORK_JOB = "gate (GitHub-hosted fork)"
REQUIRED_HOSTED_FORK_STEPS = {
    "Check out Abbey",
    "Check out the verified ABI dependency",
    "Check out the public WDBX substrate",
    "Install pinned ABI toolchain",
    "Install pinned Abbey toolchain",
    "Build the real ABI binary",
    "Gate all portable Abbey modes",
}
QUALIFYING_JOBS = {
    PRIMARY_JOB: REQUIRED_PRIMARY_STEPS,
    MACOS_JOB: REQUIRED_PRIMARY_STEPS,
    HOSTED_FORK_JOB: REQUIRED_HOSTED_FORK_STEPS,
}

def run_is_real_evidence(
    run: dict, jobs: list[dict], expected_head_sha: str
) -> tuple[bool, str]:
    """Return (is_evidence, reason) for one workflow run.

    Evidence requires jobs that actually ran and succeeded. A run with no jobs,
    in-progress jobs, or incomplete status never reached completion.
    """
    # Check if run is still in progress based on status field
    status = run.get("status")
    if status != "completed":
        return False, f"run still in progress: status is {status!r}"
    workflow_path = str(run.get("path", "")).split("@", 1)[0]
    if workflow_path != RUST_WORKFLOW_PATH:
        return False, f"run is for unexpected workflow path {workflow_path!r}"
    if run.get("head_sha") != expected_head_sha:
        return False, "run head SHA does not match the checked-out revision"
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

    qualifying = [job for job in executed if job.get("name") in QUALIFYING_JOBS]
    if not qualifying:
        return False, "expected at least one executed qualifying Abbey gate job"

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

    for job in qualifying:
        job_name = str(job.get("name"))
        successful_step_names = {
            str(step.get("name"))
            for step in job.get("steps", [])
            if step.get("conclusion") == "success"
        }
        missing = sorted(QUALIFYING_JOBS[job_name] - successful_step_names)
        if missing:
            return False, (
                f"qualifying gate {job_name!r} is missing successful required steps: "
                f"{', '.join(missing)}"
            )

    if run.get("conclusion") != "success":
        return False, f"completed run did not succeed: conclusion is {run.get('conclusion')!r}"

    names = ", ".join(str(job.get("name")) for job in qualifying)
    return True, (
        f"{len(executed)} job(s) executed and succeeded with "
        f"{successful_steps} successful step(s); qualifying gate(s): {names}"
    )


def main(argv: list[str]) -> int:
    if len(argv) != 2:
        print("usage: require_executed_run.py <run-id>", file=sys.stderr)
        return 2
    run_id = argv[1]
    repo = "donaldfilimon/abbey"
    expected_head_sha = subprocess.run(
        ["git", "rev-parse", "HEAD"], capture_output=True, text=True, check=True
    ).stdout.strip()
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
    ok, reason = run_is_real_evidence(run, jobs, expected_head_sha)
    print(f"{'EVIDENCE' if ok else 'NOT EVIDENCE'}: {reason}")
    return 0 if ok else 1


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
