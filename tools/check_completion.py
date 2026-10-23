#!/usr/bin/env python3
"""Check completion coverage and evidence bookkeeping; never execute a plan.

The capability registry remains authoritative. References in this matrix are
review inputs, not independently authenticated proof. In particular, a green
coverage check does not qualify a capability or close the Completion Program.
"""

from __future__ import annotations

import json
from pathlib import Path
import re
import sys

ROOT = Path(__file__).resolve().parents[1]
MILESTONE_CHANNELS = (
    "source", "automated", "macos_arm64", "ubuntu_arm64", "windows11_arm",
)
STATES = {"open", "in_progress", "implemented", "blocked", "complete"}
EVIDENCE_STATES = {"required", "passed", "failed", "blocked", "not_required"}


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError(f"duplicate JSON key: {key}")
        result[key] = value
    return result


def load(root: Path = ROOT) -> dict:
    return json.loads(
        (root / "tasks/completion.json").read_text(encoding="utf-8"),
        object_pairs_hook=unique_object,
    )


def strings(value) -> bool:
    return isinstance(value, list) and bool(value) and all(
        isinstance(item, str) and bool(item.strip()) for item in value
    )


def validate(matrix: dict, root: Path = ROOT) -> list[str]:
    errors = []
    if not isinstance(matrix, dict) or set(matrix) != {
        "schema_version", "milestones", "requirements", "goals",
    } or matrix.get("schema_version") != 1:
        return ["invalid completion matrix schema"]

    def records(key, maximum):
        values = matrix[key]
        if not isinstance(values, list) or not 1 <= len(values) <= maximum:
            errors.append(f"invalid {key} collection")
            return []
        result = []
        seen = set()
        for value in values:
            if not isinstance(value, dict):
                errors.append(f"invalid {key} record")
                continue
            identifier = value.get("id")
            if not isinstance(identifier, str) or not re.fullmatch(r"[a-z][a-z0-9-]{0,79}", identifier):
                errors.append(f"invalid {key} id")
                continue
            if identifier in seen:
                errors.append(f"duplicate {'requirement' if key == 'requirements' else 'milestone'}: {identifier}")
            seen.add(identifier)
            for field in ("owner",):
                if not isinstance(value.get(field), str) or not value[field].strip():
                    errors.append(f"{identifier}: missing {field}")
            if not strings(value.get("acceptance")):
                errors.append(f"{identifier}: missing acceptance")
            if value.get("status") not in STATES:
                errors.append(f"{identifier}: invalid status")
            required = value.get("required_evidence")
            minimum = MILESTONE_CHANNELS if key == "milestones" else ("source", "automated")
            if not strings(required) or len(set(required)) != len(required) or not set(minimum).issubset(required):
                errors.append(f"{identifier}: invalid required evidence channels")
            evidence = value.get("evidence")
            if not isinstance(evidence, dict):
                errors.append(f"{identifier}: invalid evidence")
            else:
                for channel, item in evidence.items():
                    if not isinstance(item, dict) or set(item) != {"status", "refs"}:
                        errors.append(f"{identifier}: invalid {channel} evidence")
                    elif item["status"] not in EVIDENCE_STATES or not strings(item["refs"]):
                        errors.append(f"{identifier}: {channel} evidence needs a status and reference")
            result.append(value)
        return result

    milestones = records("milestones", 64)
    requirements = records("requirements", 512)
    by_id = {m["id"]: m for m in milestones}
    bindings = {}
    todo_lines = []
    for number, line in enumerate((root / "tasks/todo.md").read_text(encoding="utf-8").splitlines(), 1):
        match = re.match(r"^\s*- \[([ xX])\] (.+)$", line)
        if match:
            todo_lines.append((match[1] == " ", match[2].strip(), number))
    all_todos = {text for _, text, _ in todo_lines}
    open_todos = {text for opened, text, _ in todo_lines if opened}

    def passed(record, channel):
        evidence = record.get("evidence")
        item = evidence.get(channel) if isinstance(evidence, dict) else None
        return isinstance(item, dict) and item.get("status") == "passed" and strings(item.get("refs"))

    for requirement in requirements:
        identifier = requirement["id"]
        if requirement.get("milestone") not in by_id:
            errors.append(f"{identifier}: unknown milestone")
        todos = requirement.get("todos")
        if not isinstance(todos, list) or any(not isinstance(t, str) for t in todos):
            errors.append(f"{identifier}: invalid todos")
            continue
        for text in todos:
            if text not in all_todos:
                errors.append(f"{identifier}: stale todo: {text}")
            if text in bindings:
                errors.append(f"todo has multiple requirements: {text}")
            bindings[text] = identifier
            if requirement.get("status") == "complete" and text in open_todos:
                errors.append(f"{identifier}: complete with open todo: {text}")
        if requirement.get("status") == "complete":
            for channel in requirement.get("required_evidence", ("source", "automated")):
                if not passed(requirement, channel):
                    errors.append(f"{identifier}: complete without passed {channel} evidence")
    for opened, text, number in todo_lines:
        if opened and text not in bindings:
            errors.append(f"unmapped todo at tasks/todo.md:{number}: {text}")

    visiting, visited = set(), set()

    def visit(identifier):
        if identifier in visiting:
            errors.append(f"milestone dependency cycle: {identifier}")
            return
        if identifier in visited:
            return
        visiting.add(identifier)
        milestone = by_id[identifier]
        dependencies = milestone.get("depends_on")
        if not isinstance(dependencies, list) or any(not isinstance(d, str) for d in dependencies):
            errors.append(f"{identifier}: invalid dependencies")
            dependencies = []
        for dependency in dependencies:
            if dependency not in by_id:
                errors.append(f"{identifier}: unknown dependency {dependency}")
            else:
                visit(dependency)
                if milestone.get("status") == "complete" and by_id[dependency].get("status") != "complete":
                    errors.append(f"{identifier}: complete with unfinished dependency {dependency}")
        visiting.remove(identifier)
        visited.add(identifier)
        if milestone.get("status") == "complete":
            for channel in milestone.get("required_evidence", MILESTONE_CHANNELS):
                if not passed(milestone, channel):
                    errors.append(f"{identifier}: complete without passed {channel} evidence")
            for requirement in requirements:
                if requirement.get("milestone") == identifier and requirement.get("status") != "complete":
                    errors.append(f"{identifier}: unfinished requirement {requirement['id']}")

    for identifier in by_id:
        visit(identifier)

    goals = matrix["goals"]
    if not isinstance(goals, dict):
        return errors + ["invalid goal mappings"]
    live_goals = {}
    for block in re.findall(r"<!-- abbey-goal\s*\n(.*?)-->", (root / "tasks/goals.md").read_text(encoding="utf-8"), re.S):
        metadata = dict(line.split(": ", 1) for line in block.splitlines() if ": " in line)
        live_goals[metadata.get("id")] = metadata.get("status")
    for identifier, status in live_goals.items():
        if status != "done" and identifier not in goals:
            errors.append(f"unmapped goal: {identifier}")
    for identifier, targets in goals.items():
        if identifier not in live_goals or not strings(targets) or any(t not in by_id for t in targets):
            errors.append(f"invalid goal mapping: {identifier}")
    return errors


def main() -> int:
    try:
        matrix = load()
        errors = validate(matrix)
    except (OSError, ValueError, TypeError) as error:
        print(f"completion coverage: {error}", file=sys.stderr)
        return 1
    if errors:
        print("\n".join(errors), file=sys.stderr)
        return 1
    print(f"completion coverage: {len(matrix['requirements'])} requirements, {len(matrix['milestones'])} milestones; coverage only, not completion evidence")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
