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

    Evidence requires jobs that actually ran. A run with no jobs never
    reached a runner, whatever its conclusion says.
    """
    executed = [j for j in jobs if j.get("conclusion") not in (None, "skipped")]
    if not jobs:
        return False, (
            f"zero jobs scheduled (run conclusion={run.get('conclusion')!r}); "
            "infrastructure evidence only, not source-test evidence"
        )
    if not executed:
        names = ", ".join(str(j.get("name")) for j in jobs)
        return False, f"all {len(jobs)} job(s) skipped ({names}); nothing executed"
    failed = [j for j in executed if j.get("conclusion") != "success"]
    if failed:
        names = ", ".join(str(j.get("name")) for j in failed)
        return False, (
            f"{len(executed)} job(s) executed but {len(failed)} failed ({names}); "
            "real execution evidence, not a passing gate"
        )
    return True, f"{len(executed)} job(s) executed and succeeded"


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
