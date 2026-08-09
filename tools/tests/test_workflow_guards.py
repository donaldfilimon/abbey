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


def job_field(body: str, field: str) -> str:
    """Return one actual job-level field, including folded scalar content."""
    lines = body.splitlines()
    pattern = re.compile(rf"^ {{4}}{re.escape(field)}:\s*(.*)$")
    for index, line in enumerate(lines):
        match = pattern.match(line)
        if match is None:
            continue
        value = match.group(1).strip()
        if value not in (">", "|", ">-", "|-"):
            return value
        continuation = []
        for nested in lines[index + 1 :]:
            if nested.strip() and len(nested) - len(nested.lstrip()) <= 4:
                break
            if nested.strip() and not nested.lstrip().startswith("#"):
                continuation.append(nested.strip())
        return " ".join(continuation)
    raise AssertionError(f"job is missing field {field!r}")


class WorkflowGuards(unittest.TestCase):
    def setUp(self) -> None:
        self.text = WORKFLOW.read_text(encoding="utf-8")
        self.jobs = job_blocks(self.text)

    def test_workflow_declares_jobs(self) -> None:
        self.assertTrue(self.jobs, "no jobs parsed out of rust.yml")

    def test_every_job_runs_only_on_self_hosted_runners(self) -> None:
        for name, body in self.jobs.items():
            self.assertIn(
                "self-hosted",
                job_field(body, "runs-on"),
                f"job {name!r} must use the owned self-hosted runner pool",
            )

    def test_every_self_hosted_job_requires_same_repository(self) -> None:
        for name, body in self.jobs.items():
            self.assertIn(
                "github.repository == 'donaldfilimon/abbey'",
                job_field(body, "if"),
                f"self-hosted job {name!r} must refuse foreign repositories",
            )

    def test_every_self_hosted_job_is_gated_on_runner_availability(self) -> None:
        # A self-hosted job whose runner label is not registered cannot be
        # scheduled. Each such job must be skippable via a repository variable
        # so the workflow degrades to "skipped", never to an unschedulable job.
        expected = {
            "gate-linux": "vars.ABBEY_LINUX_ARM64_RUNNER == 'enabled'",
            "gate-macos": "vars.ABBEY_MACOS_ARM64_RUNNER == 'enabled'",
        }
        self.assertEqual(set(self.jobs), set(expected))
        for name, body in self.jobs.items():
            self.assertIn(
                expected[name],
                job_field(body, "if"),
                f"self-hosted job {name!r} needs its exact availability guard",
            )

    def test_every_job_contains_required_release_inventory_and_cleanup_steps(self) -> None:
        required = {
            "Gate both Abbey feature sets",
            "RustSec dependency scan",
            "Warning-denied API docs and release binary",
            "Release install and provider inventory smoke",
            "ABI-backed local mesh proof",
            "Clean runner workspace",
        }
        for name, body in self.jobs.items():
            steps = set(re.findall(r"^ {6}- name: (.+)$", body, re.MULTILINE))
            self.assertFalse(required - steps, f"job {name!r} is missing required steps")
            for isolated in (
                'export HOME="$smoke_root/home"',
                'export XDG_CONFIG_HOME="$smoke_root/home/.config"',
                'export XDG_STATE_HOME="$smoke_root/home/.local/state"',
                'export ABBEY_CONFIG="$smoke_root/home/.config/abbey/config.toml"',
                'export ABBEY_STATE_DIR="$smoke_root/state"',
            ):
                self.assertIn(isolated, body, f"job {name!r} lacks isolated smoke state")
            self.assertRegex(
                body,
                r"(?m)^ {6}- name: Clean runner workspace\n {8}if: always\(\)$",
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
            condition = job_field(body, "if")
            if "pull_request" not in condition:
                continue
            self.assertIn(
                "github.event.pull_request.head.repo.full_name == github.repository",
                condition,
                f"job {name!r} accepts pull_request without pinning the head repo",
            )

    def test_no_job_widens_token_permissions(self) -> None:
        # Job-level `permissions:` fully overrides the workflow-level default
        # for that job rather than intersecting with it, so a single job block
        # can silently widen the token beyond `contents: read`.
        for name, body in self.jobs.items():
            self.assertNotRegex(
                body,
                r"(?m)^ {4}permissions\s*:",
                f"job {name!r} must not override the workflow token permissions",
            )

    def test_every_job_level_permissions_syntax_is_detected(self) -> None:
        forbidden = (
            "    permissions: write-all",
            "    permissions: read-all",
            "    permissions: {contents: read, pull-requests: write}",
            "    permissions:\n      contents: write",
        )
        for body in forbidden:
            self.assertRegex(body, r"(?m)^ {4}permissions\s*:")
