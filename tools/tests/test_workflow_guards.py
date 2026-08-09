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


class ForkSafety(unittest.TestCase):
    def setUp(self) -> None:
        self.text = WORKFLOW.read_text(encoding="utf-8")
        self.jobs = job_blocks(self.text)

    def test_no_pull_request_target_trigger(self) -> None:
        # pull_request_target runs with repository secrets against fork code.
        self.assertNotIn("pull_request_target", self.text)

    def test_token_permissions_are_read_only(self) -> None:
        self.assertIsNotNone(
            re.search(r"^permissions:\n  contents: read\s*$", self.text, re.MULTILINE),
            "workflow must declare exactly `permissions:\\n  contents: read`",
        )

    def test_pull_request_jobs_require_a_same_repo_head(self) -> None:
        for name, body in self.jobs.items():
            if "pull_request" not in body:
                continue
            self.assertIn(
                "github.event.pull_request.head.repo.full_name == github.repository",
                body,
                f"job {name!r} accepts pull_request without pinning the head repo",
            )

    def test_no_hosted_runner_reintroduced(self) -> None:
        # Hosted runners are the assumption Phase 2 replaces; a silent
        # `ubuntu-latest` would re-add a runner that cannot see the pinned ABI
        # checkout layout this repo needs.
        self.assertNotIn("ubuntu-latest", self.text)
        self.assertNotIn("macos-latest", self.text)

    def test_no_job_widens_token_permissions(self) -> None:
        # Job-level `permissions:` fully overrides the workflow-level default
        # for that job rather than intersecting with it, so a single job block
        # can silently widen the token beyond `contents: read`.
        for name, body in self.jobs.items():
            for match in re.finditer(r"^(\s*)permissions:\s*$", body, re.MULTILINE):
                parent_indent = len(match.group(1))
                # Collect all permission lines that follow, stopping at dedent
                permissions_dict = {}
                tail = body[match.end():].lstrip("\n")
                for line in tail.split("\n"):
                    if not line.strip():
                        continue
                    current_indent = len(line) - len(line.lstrip())
                    if current_indent <= parent_indent:
                        # We've exited the permissions block
                        break
                    # This line is part of the permissions block
                    if ":" in line:
                        key, val = line.split(":", 1)
                        key = key.strip()
                        val = val.strip()
                        permissions_dict[key] = val
                # Assert it's exactly {"contents": "read"}
                self.assertEqual(
                    permissions_dict,
                    {"contents": "read"},
                    f"job {name!r} has permissions {permissions_dict!r} instead of "
                    f"exactly {{'contents': 'read'}}",
                )
