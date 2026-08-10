# Phase 2: Self-Hosted CI That Actually Executes — Implementation Plan

> **Historical status (2026-08-10): repo-side implementation complete.** The
> twenty implementation steps below are checked as a record of work already
> landed; they are not active backlog. Hosted execution itself remains an
> external evidence prerequisite and is tracked only by the unchecked Phase 2
> item in `tasks/todo.md` plus `tools/ci/require_executed_run.py`.
>
> **Historical execution note:** this plan originally required an agentic
> execution skill. It is retained for provenance; do not execute it again.

**Goal:** Make Abbey's CI workflow structurally incapable of the two failure
modes it currently has — a self-hosted job that targets a runner label nobody
guarded, and a zero-job `startup_failure` being mistaken for a passing gate —
and give Phase 2 a machine-checkable closure test.

**Architecture:** Three repo-side units, all stdlib Python enforced by the
existing `check.sh` step `python3 -m unittest discover -s tools/tests`. A
workflow-invariant test parses `.github/workflows/rust.yml` as text and asserts
the safety guards every self-hosted job must carry. A run-evidence module turns
"did CI actually execute?" into a pure function over the GitHub API's run JSON,
so the Phase 2 evidence bar stops being a human judgement call. The workflow
itself is edited only to satisfy the tests.

**Tech Stack:** Python 3.14 standard library only (`unittest`, `pathlib`, `re`,
`json`) · GitHub Actions YAML · `gh` CLI for the one manual evidence capture.

## Global Constraints

Copied verbatim from `tasks/todo.md` Phase 2 and this repo's standing rules:

- "same-repository jobs only, no fork-originated privileged jobs, no
  `pull_request_target` for untrusted code, minimal token permissions, per-job
  cleanup"
- "Evidence bar: close the CI TODOs only after a run with actual jobs +
  successful steps; a zero-job `startup_failure` is infrastructure evidence,
  never source-test evidence; missing runners/VM images/admin permission =
  explicit external blockers"
- "Abbey CI/release builds check out that immutable ABI SHA — never floating
  ABI main." `ABI_REVISION` is currently
  `32e372d7f522f5a6c9c0ef92c5b9612b52cfea05`; no task may unpin it.
- **No third-party Python.** PyYAML is *not installed* on this machine
  (`python3 -c "import yaml"` fails) and `tools/tests/test_check_claims_sync.py`
  uses stdlib only. Any test that parses YAML must do so with stdlib text
  handling.
- `./check.sh` is the merge bar and must pass before any task is considered
  done. It runs fmt, clippy `-D warnings`, and tests for **all three** feature
  sets (default · `wdbx` · `personal-edition`), the file-size guard, and the
  `tools/tests` unittest discovery this plan extends.
- Claims honesty: nothing in this plan promotes Phase 2 to Current. Phase 2
  closes only under Task 4's recorded evidence, not because these tests pass.

## File Structure

| File | Responsibility |
|---|---|
| `tools/tests/test_workflow_guards.py` | **Create.** Parses `.github/workflows/rust.yml` and asserts the self-hosted safety invariants. Pure assertions over workflow text; no network. |
| `.github/workflows/rust.yml` | **Modify** (`gate-macos` `if:` block only). Add the runner-availability guard `gate-linux` already has. |
| `tools/ci/require_executed_run.py` | **Create.** Pure `run_is_real_evidence(run, jobs)` predicate plus a thin `__main__` that queries `gh`. Encodes "zero jobs is not evidence". |
| `tools/tests/test_require_executed_run.py` | **Create.** Unit tests for the predicate against recorded run shapes, including the exact `startup_failure`/0-job shape this repo has produced all day. |
| `tasks/todo.md` | **Modify** (Phase 2 block). Record what the tests now enforce and what stays externally blocked. |

Files that change together live together: both new tests sit in `tools/tests/`
beside the existing one, and `check.sh` picks them up with no edit because it
already discovers `test_*.py`.

---

### Task 1: Workflow guard invariants (and fix the `gate-macos` asymmetry)

**Files:**
- Create: `tools/tests/test_workflow_guards.py`
- Modify: `.github/workflows/rust.yml` (the `gate-macos` `if:` expression)

**Interfaces:**
- Consumes: nothing from earlier tasks.
- Produces: `job_blocks(text: str) -> dict[str, str]` — splits the workflow's
  `jobs:` mapping into `{job_name: block_text}`. Task 2 imports this exact
  function from this module.

**Context an implementer needs:** `gate-linux` is guarded by
`vars.ABBEY_LINUX_ARM64_RUNNER == 'enabled'`, so it is skipped when no Linux
runner is registered. `gate-macos` has no equivalent guard, so it is always
requested. A job requesting a `[self-hosted, macOS, ARM64, abbey]` label that
no registered runner carries cannot be scheduled. The fix is symmetry, not
deletion — the macOS adjunct is wanted, just gated.

- [x] **Step 1: Write the failing test**

Create `tools/tests/test_workflow_guards.py`:

```python
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
```

- [x] **Step 2: Run the test to verify it fails**

Run: `python3 -m unittest tools.tests.test_workflow_guards -v`
(from the repository root)

Expected: `test_every_self_hosted_job_is_gated_on_runner_availability` FAILS
with `self-hosted job 'gate-macos' needs a vars.<RUNNER> == 'enabled' guard`.
The other three tests pass — they describe guards that already exist.

If `gate-macos` unexpectedly passes, stop and re-read the workflow: someone
else has changed it, and this plan's premise needs re-verification.

- [x] **Step 3: Make the minimal workflow change**

In `.github/workflows/rust.yml`, inside the `gate-macos` job, change:

```yaml
    if: >
      github.repository == 'donaldfilimon/abbey' &&
```

to:

```yaml
    if: >
      vars.ABBEY_MACOS_ARM64_RUNNER == 'enabled' &&
      github.repository == 'donaldfilimon/abbey' &&
```

Leave the rest of the expression exactly as it is. Do not touch `gate-linux`.

- [x] **Step 4: Run the test to verify it passes**

Run: `python3 -m unittest tools.tests.test_workflow_guards -v`
Expected: 4 tests, all PASS.

- [x] **Step 5: Run the full gate**

Run: `./check.sh`
Expected: ends with `check.sh: OK`. The new file is picked up automatically by
the existing `python3 -m unittest discover -s tools/tests -p 'test_*.py'` step —
confirm that step's output count went up by 4.

- [x] **Step 6: Commit**

```bash
git add tools/tests/test_workflow_guards.py .github/workflows/rust.yml
git commit -m "ci: gate the macOS adjunct on runner availability, enforced by test

gate-linux was guarded by vars.ABBEY_LINUX_ARM64_RUNNER but gate-macos
was not, so every same-repo event requested a [self-hosted, macOS,
ARM64, abbey] runner whether or not one is registered. Adds the missing
guard and a stdlib workflow-invariant test (same-repository, runner
availability) so the asymmetry cannot come back."
```

---

### Task 2: Fork-safety invariants the workflow must keep

**Files:**
- Modify: `tools/tests/test_workflow_guards.py` (append a second test class)

**Interfaces:**
- Consumes: `job_blocks` from Task 1.
- Produces: nothing consumed later.

**Context:** Phase 2's spec forbids `pull_request_target` for untrusted code and
requires minimal token permissions. These already hold; this task freezes them
so a future edit cannot quietly reintroduce a fork-code-on-local-machine hole.
The known-and-deliberate gap — no CI gate at all for fork PRs, since
`ubuntu-latest` was removed — is asserted *as documented*, not silently
tolerated.

- [x] **Step 1: Write the failing test**

Append to `tools/tests/test_workflow_guards.py`:

```python
class ForkSafety(unittest.TestCase):
    def setUp(self) -> None:
        self.text = WORKFLOW.read_text(encoding="utf-8")
        self.jobs = job_blocks(self.text)

    def test_no_pull_request_target_trigger(self) -> None:
        # pull_request_target runs with repository secrets against fork code.
        self.assertNotIn("pull_request_target", self.text)

    def test_token_permissions_are_read_only(self) -> None:
        self.assertRegex(self.text, r"^permissions:\n  contents: read$", )

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
```

Note the `test_token_permissions_are_read_only` regex needs `re.MULTILINE`.
Write it as:

```python
    def test_token_permissions_are_read_only(self) -> None:
        self.assertIsNotNone(
            re.search(r"^permissions:\n  contents: read\s*$", self.text, re.MULTILINE),
            "workflow must declare exactly `permissions:\\n  contents: read`",
        )
```

Use that second form; delete the first.

- [x] **Step 2: Run the tests**

Run: `python3 -m unittest tools.tests.test_workflow_guards -v`
Expected: 8 tests, all PASS — these assert invariants the workflow already
satisfies. This task is a ratchet, not a repair. If any fails, the workflow has
drifted from the Phase 2 spec and that is the actual finding: stop and report
it rather than loosening the test.

- [x] **Step 3: Run the full gate**

Run: `./check.sh`
Expected: `check.sh: OK`.

- [x] **Step 4: Commit**

```bash
git add tools/tests/test_workflow_guards.py
git commit -m "ci: freeze the fork-safety invariants Phase 2 requires

Asserts no pull_request_target, read-only token permissions, same-repo
head for every pull_request job, and no hosted-runner reintroduction.
All already true; the test stops them regressing."
```

---

### Task 3: Make "CI actually executed" a testable predicate

**Files:**
- Create: `tools/ci/require_executed_run.py`
- Create: `tools/tests/test_require_executed_run.py`

**Interfaces:**
- Consumes: nothing from earlier tasks.
- Produces: `run_is_real_evidence(run: dict, jobs: list[dict]) -> tuple[bool, str]`
  — returns `(is_evidence, reason)`. Task 4 quotes its reason strings in the
  ledger.

**Context:** Every Actions run on this repository so far has ended
`startup_failure` at 0s with zero jobs scheduled, including GitHub's own default
template and a Dependabot run. The Phase 2 spec is explicit that this is
infrastructure evidence, never source-test evidence. That distinction is
currently a sentence in a Markdown file; this task makes it executable.

- [x] **Step 1: Write the failing test**

Create `tools/tests/test_require_executed_run.py`:

```python
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
        ok, reason = module.run_is_real_evidence({"conclusion": "success"}, [])
        self.assertFalse(ok)
        self.assertIn("zero jobs", reason)

    def test_failed_steps_are_execution_evidence_but_not_a_pass(self) -> None:
        run = {"conclusion": "failure"}
        jobs = [{"name": "gate (Linux ARM64)", "conclusion": "failure", "steps": [
            {"name": "Gate both Abbey feature sets", "conclusion": "failure"}
        ]}]
        ok, reason = module.run_is_real_evidence(run, jobs)
        self.assertFalse(ok)
        self.assertIn("executed", reason)

    def test_executed_and_successful_run_is_evidence(self) -> None:
        run = {"conclusion": "success"}
        jobs = [{"name": "gate (Linux ARM64)", "conclusion": "success", "steps": [
            {"name": "Gate both Abbey feature sets", "conclusion": "success"}
        ]}]
        ok, reason = module.run_is_real_evidence(run, jobs)
        self.assertTrue(ok, reason)
        self.assertIn("1 job", reason)

    def test_skipped_jobs_do_not_count_as_execution(self) -> None:
        # A runner-availability guard that skips every job looks like a green
        # run but proves nothing about the gate.
        run = {"conclusion": "success"}
        jobs = [{"name": "gate (macOS ARM64 adjunct)", "conclusion": "skipped", "steps": []}]
        ok, reason = module.run_is_real_evidence(run, jobs)
        self.assertFalse(ok)
        self.assertIn("skipped", reason)
```

- [x] **Step 2: Run the test to verify it fails**

Run: `python3 -m unittest tools.tests.test_require_executed_run -v`
Expected: collection error — `FileNotFoundError` / module load failure, because
`tools/ci/require_executed_run.py` does not exist yet.

- [x] **Step 3: Write the minimal implementation**

Create `tools/ci/require_executed_run.py`:

```python
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
```

- [x] **Step 4: Run the tests to verify they pass**

Run: `python3 -m unittest tools.tests.test_require_executed_run -v`
Expected: 5 tests, all PASS.

- [x] **Step 5: Run the full gate**

Run: `./check.sh`
Expected: `check.sh: OK`, with the unittest step now discovering both new test
modules.

- [x] **Step 6: Commit**

```bash
git add tools/ci/require_executed_run.py tools/tests/test_require_executed_run.py
git commit -m "ci: make 'the run actually executed' a testable predicate

Phase 2 says a zero-job startup_failure is infrastructure evidence, not
source-test evidence. That rule now lives in code: run_is_real_evidence
rejects zero-job runs, all-skipped runs, and failed-but-executed runs
with distinct reasons, so a green-looking summary cannot close the
phase."
```

---

### Task 4: Record what is enforced and what stays externally blocked

**Files:**
- Modify: `tasks/todo.md` (the Phase 2 block only)

**Interfaces:**
- Consumes: the reason strings from `run_is_real_evidence` (Task 3).
- Produces: nothing.

**Context:** The ledger contract forbids closing a phase on a green local gate.
Tasks 1–3 add enforcement, not evidence of a working runner. This task records
that distinction so the next reader does not mistake passing tests for CI.

- [x] **Step 1: Update the Phase 2 checklist**

In `tasks/todo.md`, inside the `- [ ] **Phase 2 …**` block, append these
sub-bullets (keep the parent checkbox **unchecked**):

```markdown
  - Enforced 2026-08-08 (repo-side, no runner yet): `tools/tests/test_workflow_guards.py`
    asserts every self-hosted job carries both a same-repository guard and a
    `vars.<RUNNER> == 'enabled'` availability guard — `gate-macos` was missing the
    latter and always requested an unregistered `[self-hosted, macOS, ARM64, abbey]`
    label; plus no `pull_request_target`, read-only token permissions, same-repo head
    for pull_request jobs, and no hosted-runner reintroduction.
    `tools/ci/require_executed_run.py` makes the evidence bar executable:
    zero-job, all-skipped, and failed-but-executed runs each return a distinct
    non-evidence reason. Both run under `check.sh`'s existing `tools/tests` discovery.
  - Still externally blocked, unchanged by the above: no self-hosted runner is
    registered, and every run to date ends `startup_failure` at 0s with zero jobs
    (including GitHub's own default template and a Dependabot run), so no workflow
    content has ever executed. Owner action: register an Ubuntu ARM64 runner with
    labels `self-hosted,Linux,ARM64,abbey`, set repository variable
    `ABBEY_LINUX_ARM64_RUNNER=enabled` (and `ABBEY_MACOS_ARM64_RUNNER=enabled` for the
    adjunct), then confirm Actions can schedule at all — a private repository with no
    Actions minutes cannot start a run regardless of runner state.
  - Closure test for this phase: `python3 tools/ci/require_executed_run.py <run-id>`
    prints `EVIDENCE: N job(s) executed and succeeded` and exits 0. Until that
    command succeeds against a real run id, Phase 2 stays unchecked.
```

- [x] **Step 2: Verify the ledger still parses**

Run: `python3 tools/check_claims_sync.py`
Expected: `claims/docs: OK (41 claims, schema 1, sha256 …)` — the generator must
not report drift. Phase 2 is a `todo.md` checklist item, not a claim, so the
digest should be unchanged. If it reports drift, run
`python3 tools/check_claims_sync.py --write` and inspect the diff before
committing.

- [x] **Step 3: Run the full gate**

Run: `./check.sh`
Expected: `check.sh: OK`.

- [x] **Step 4: Commit**

```bash
git add tasks/todo.md
git commit -m "docs(ledger): record Phase 2 enforcement and the external blocker

Tasks 1-3 add enforcement, not a working runner. Phase 2 stays
unchecked: no self-hosted runner is registered and no run has ever
scheduled a job. Records the exact owner action and the closure test
(require_executed_run.py against a real run id)."
```

---

## External prerequisites (not automatable from this repo)

These are owner actions. No task above can substitute for them, and none of
them should be faked with a green local gate:

1. **Confirm Actions can schedule at all.** Every run on this private repo ends
   `startup_failure` at 0s with zero jobs — including GitHub's own default
   template, which is known-valid YAML. The same account's *public* `abi` repo
   schedules and executes jobs normally (18–40s durations), which isolates the
   cause to the private repository rather than workflow content. Check
   github.com/settings/billing → Actions minutes and spending limit.
2. **Register the Ubuntu ARM64 runner** with labels exactly
   `self-hosted,Linux,ARM64,abbey`, then set repository variable
   `ABBEY_LINUX_ARM64_RUNNER=enabled`.
3. **Optionally register the macOS adjunct** with labels
   `self-hosted,macOS,ARM64,abbey` and set `ABBEY_MACOS_ARM64_RUNNER=enabled`.
   After Task 1 this job stays skipped until that variable exists, so leaving it
   unset is safe.
4. **Win11 ARM evidence runner** stays an explicit external blocker — the Phase 2
   spec gates it on "provisioning/signing prerequisites", which do not exist yet.

## Self-review

**Spec coverage.** Phase 2's bullets map as follows. *Same-repository only / no
fork-originated privileged jobs / no `pull_request_target` / minimal token
permissions* → Task 2. *Runner labels for Linux ARM64 primary + macOS adjunct*
→ Task 1 (gating) plus External prerequisites 2–3 (registration). *Win11 ARM
runner* → External prerequisite 4, explicitly blocked. *CI checks out Abbey at
workflow revision + ABI at the recorded merged commit, runs `check.sh`,
rustdoc, release/install smoke, `ABBEY_ABI_BIN` smoke, plugin/MCP inventory* →
already implemented in the current `rust.yml`; no task changes it, and the
Global Constraints forbid unpinning `ABI_REVISION`. *Open Codex P2 — no CI gate
for fork PRs* → Task 2's `test_no_hosted_runner_reintroduced` freezes the
current deliberate state; closing the gap needs a hosted runner, which
contradicts Phase 2's premise, so it stays recorded rather than "fixed".
*Evidence bar* → Task 3 (executable) and Task 4 (recorded).

**Placeholder scan.** No TBD/TODO/"handle edge cases"/"similar to Task N". Every
code step carries complete, runnable content. Task 2's first draft of
`test_token_permissions_are_read_only` is shown and then explicitly replaced —
the implementer is told to use the second form and delete the first, so no
broken regex ships.

**Type consistency.** `job_blocks(text) -> dict[str, str]` is defined in Task 1
and imported by Task 2 under the same name. `run_is_real_evidence(run, jobs) ->
tuple[bool, str]` is defined in Task 3 and used identically in its tests and in
`main`. Repository variable names are consistent throughout:
`ABBEY_LINUX_ARM64_RUNNER` and `ABBEY_MACOS_ARM64_RUNNER`.

**Known risk.** Task 1's parser is text-based because PyYAML is unavailable. It
assumes the workflow keeps two-space indentation under `jobs:` — true today and
enforced by `test_workflow_declares_jobs` failing loudly if the shape changes,
rather than silently passing zero jobs.
