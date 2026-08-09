from __future__ import annotations

from pathlib import Path
import re
import unittest

WORKFLOW = Path(__file__).resolve().parents[2] / ".github" / "workflows" / "rust.yml"


def job_blocks(text: str) -> dict[str, str]:
    """Split the workflow's `jobs:` mapping into {job_name: block_text}.

    Stdlib only: PyYAML is deliberately not a dependency of this repo's
    tooling, and check.sh runs these tests on a bare runner.
    """
    lines = text.splitlines()
    try:
        start = next(i for i, line in enumerate(lines) if line.rstrip() == "jobs:")
    except StopIteration as exc:  # pragma: no cover - guarded by test below
        raise AssertionError("workflow has no top-level `jobs:` mapping") from exc

    jobs: dict[str, list[str]] = {}
    current: str | None = None
    for line in lines[start + 1 :]:
        stripped = line.strip()
        if not stripped or stripped.startswith("#"):
            if current is not None:
                jobs[current].append(line)
            continue
        indent = len(line) - len(line.lstrip())
        if indent == 0:
            break  # left the jobs mapping
        header = re.match(r"^ {2}(?P<name>[A-Za-z0-9_-]+):\s*$", line)
        if header is not None and indent == 2:
            current = header.group("name")
            jobs[current] = []
            continue
        if current is not None:
            jobs[current].append(line)
    return {name: "\n".join(body) for name, body in jobs.items()}


class WorkflowGuards(unittest.TestCase):
    def setUp(self) -> None:
        self.text = WORKFLOW.read_text(encoding="utf-8")
        self.jobs = job_blocks(self.text)

    def test_workflow_declares_jobs(self) -> None:
        self.assertTrue(self.jobs, "no jobs parsed out of rust.yml")

    def self_hosted_jobs(self) -> dict[str, str]:
        return {n: b for n, b in self.jobs.items() if "self-hosted" in b}

    def test_self_hosted_jobs_exist(self) -> None:
        self.assertTrue(
            self.self_hosted_jobs(),
            "expected at least one self-hosted job; hosted runners are the "
            "broken assumption Phase 2 replaces",
        )

    def test_every_self_hosted_job_requires_same_repository(self) -> None:
        for name, body in self.self_hosted_jobs().items():
            self.assertIn(
                "github.repository == 'donaldfilimon/abbey'",
                body,
                f"self-hosted job {name!r} must refuse foreign repositories",
            )

    def test_every_self_hosted_job_is_gated_on_runner_availability(self) -> None:
        # A self-hosted job whose runner label is not registered cannot be
        # scheduled. Each such job must be skippable via a repository variable
        # so the workflow degrades to "skipped", never to an unschedulable job.
        for name, body in self.self_hosted_jobs().items():
            self.assertRegex(
                body,
                r"vars\.[A-Z0-9_]+ == 'enabled'",
                f"self-hosted job {name!r} needs a vars.<RUNNER> == 'enabled' guard",
            )
