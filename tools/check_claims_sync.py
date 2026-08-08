#!/usr/bin/env python3
"""Validate or refresh Abbey's generated capability-ledger documentation."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile
from typing import Any


ROOT = Path(__file__).resolve().parent.parent
GENERATED_LEDGER = ROOT / "docs/claims.md"
DOCUMENT_MODES = {
    ROOT / "AGENTS.md": "table",
    ROOT / "CLAUDE.md": "summary",
    ROOT / "README.md": "summary",
    ROOT / "tasks/goals.md": "summary",
    ROOT / "tasks/todo.md": "summary",
    ROOT / "tasks/lessons.md": "summary",
    ROOT / "docs/architecture.md": "summary",
    ROOT / "docs/identity.md": "table",
    ROOT / "docs/production.md": "summary",
}
LEGACY_PREFIX = "<!-- abbey-claims-sha256: "
BEGIN_PREFIX = "<!-- BEGIN abbey-generated:claims-"
END_PREFIX = "<!-- END abbey-generated:claims-"
GOAL_OPEN = "<!-- abbey-goal"
GOAL_CLOSE = "-->"
GOAL_KEYS = {
    "id",
    "status",
    "implementation-evidence",
    "automated-test-evidence",
    "live-external-evidence",
    "next-action",
    "blocker-owner",
}
GOAL_STATUSES = {"todo", "in_progress", "blocked", "proposed", "done"}
FORBIDDEN_DRIFT = {
    ROOT / "src/deferred.rs": (
        "LoRA runners OOS",
        "local weights OOS",
        "NPU/TPU runtime OOS",
        "MCP/ACP host OOS",
    ),
    ROOT / "src/learn.rs": (
        "LoRA / fine-tune is Out of scope",
        "LoRA is out of scope",
        "oos:    LoRA runners",
        "oos:   LoRA / fine-tune",
    ),
    ROOT / "src/platform.rs": (
        "are Out of scope (`abbey claims oos`)",
        "# OOS: Abbey accelerator runtime",
        "accelerator runtime (see claims oos)",
    ),
    ROOT / "src/slash.rs": (
        "local weights OOS",
        "runners OOS",
        "bundled OOS",
        "host OOS",
    ),
    ROOT / "src/doctor.rs": ("LoRA OOS",),
    ROOT / "src/surfaces.rs": (
        "local weights OOS",
        'Abbey tool runtime", "—", "Out of scope',
    ),
    ROOT / "docs/architecture.md": (
        "weights/engine/host stay OOS",
        "OOS honesty pack — `oos`/`lora`/`weights`/`accel`/`shell`/`host`",
    ),
    ROOT / "tasks/goals.md": ("accelerator runtimes stay OOS",),
    ROOT / "CLAUDE.md": (
        "weights/engine/host OOS",
        "OOS honesty pack — lora/weights/accel/shell/host",
    ),
}


def canonical_manifest() -> tuple[dict[str, Any], str]:
    result = subprocess.run(
        ["cargo", "run", "--quiet", "--bin", "abbey", "--", "claims", "manifest"],
        cwd=ROOT,
        check=True,
        capture_output=True,
        text=True,
        timeout=120,
    )
    raw = json.loads(result.stdout)
    manifest = normalize_manifest(raw)
    canonical = json.dumps(manifest, ensure_ascii=False, separators=(",", ":"))
    return manifest, hashlib.sha256(canonical.encode()).hexdigest()


def normalize_manifest(raw: object) -> dict[str, Any]:
    if not isinstance(raw, dict):
        raise ValueError("claims manifest must be a JSON object")
    if raw.get("schema_version") != 1:
        raise ValueError("claims manifest schema_version must be 1")
    claims = raw.get("claims")
    if not isinstance(claims, list) or not claims:
        raise ValueError("claims manifest must contain a non-empty claims array")

    required = {
        "id",
        "name",
        "status",
        "note",
        "instead",
        "evidence",
        "next_action",
        "blocker_owner",
    }
    ids: set[str] = set()
    names: set[str] = set()
    statuses = {"current", "partial", "proposed", "blocked", "out_of_scope"}
    for index, claim in enumerate(claims):
        if not isinstance(claim, dict):
            raise ValueError(f"claim {index} must be an object")
        missing = required - claim.keys()
        if missing:
            raise ValueError(f"claim {index} missing fields: {sorted(missing)}")
        claim_id = claim["id"]
        name = claim["name"]
        if not isinstance(claim_id, str) or not claim_id:
            raise ValueError(f"claim {index} has an invalid id")
        if claim_id in ids:
            raise ValueError(f"duplicate claim id: {claim_id}")
        if not isinstance(name, str) or not name:
            raise ValueError(f"claim {claim_id} has an invalid name")
        if name in names:
            raise ValueError(f"duplicate claim name: {name}")
        if claim["status"] not in statuses:
            raise ValueError(f"claim {claim_id} has an unknown status")
        evidence = claim["evidence"]
        if not isinstance(evidence, dict):
            raise ValueError(f"claim {claim_id} evidence must be an object")
        evidence_keys = {
            "implementation_refs",
            "automated_test_refs",
            "local_live",
            "external_required",
        }
        if evidence.keys() != evidence_keys:
            raise ValueError(f"claim {claim_id} has invalid evidence fields")
        for refs_key in ("implementation_refs", "automated_test_refs"):
            refs = evidence[refs_key]
            if not isinstance(refs, list) or any(
                not isinstance(item, str) or not item for item in refs
            ):
                raise ValueError(f"claim {claim_id} has invalid {refs_key}")
        for state_key in ("local_live", "external_required"):
            state = evidence[state_key]
            if not isinstance(state, dict) or state.get("state") not in {
                "verified",
                "required",
                "not_required",
            }:
                raise ValueError(f"claim {claim_id} has invalid {state_key} state")
            if state["state"] in {"verified", "required"}:
                refs = state.get("refs")
                if not isinstance(refs, list) or not refs:
                    raise ValueError(f"claim {claim_id} has empty {state_key} refs")
            elif not isinstance(state.get("reason"), str) or not state["reason"]:
                raise ValueError(f"claim {claim_id} lacks {state_key} rationale")
        ids.add(claim_id)
        names.add(name)
    return raw


def markdown_cell(value: object) -> str:
    if value is None:
        return "—"
    if isinstance(value, dict):
        state = str(value.get("state", "unknown"))
        detail = value.get("refs", value.get("reason", ""))
        if isinstance(detail, list):
            detail_text = "; ".join(str(item) for item in detail)
        else:
            detail_text = str(detail)
        text = f"{state}: {detail_text}".rstrip(": ")
    elif isinstance(value, list):
        text = "; ".join(str(item) for item in value) or "—"
    else:
        text = str(value) or "—"
    return text.replace("|", "\\|").replace("\n", " ")


def claim_rows(manifest: dict[str, Any]) -> list[dict[str, Any]]:
    claims = manifest["claims"]
    assert isinstance(claims, list)
    return claims


def status_counts(manifest: dict[str, Any]) -> str:
    order = ("current", "partial", "proposed", "blocked", "out_of_scope")
    labels = {
        "current": "Current",
        "partial": "Partial",
        "proposed": "Proposed",
        "blocked": "Blocked",
        "out_of_scope": "Out of scope",
    }
    counts = {status: 0 for status in order}
    for claim in claim_rows(manifest):
        status = str(claim["status"])
        counts[status] = counts.get(status, 0) + 1
    return " · ".join(f"{counts[status]} {labels[status]}" for status in order)


def status_label(status: object) -> str:
    return {
        "current": "Current",
        "partial": "Partial",
        "proposed": "Proposed",
        "blocked": "Blocked",
        "out_of_scope": "Out of scope",
    }.get(str(status), str(status))


def parse_goal_metadata(text: str) -> list[dict[str, str]]:
    lines = text.splitlines()
    goals: list[dict[str, str]] = []
    ids: set[str] = set()
    index = 0
    while index < len(lines):
        if not lines[index].startswith("## "):
            index += 1
            continue
        title = lines[index][3:].strip()
        heading_line = index + 1
        if not title:
            raise ValueError(f"goals.md:{heading_line}: empty goal heading")
        index += 1
        if index >= len(lines) or lines[index].strip() != GOAL_OPEN:
            raise ValueError(
                f"goals.md:{heading_line}: `{title}` lacks adjacent goal metadata"
            )
        index += 1
        metadata: dict[str, str] = {"title": title}
        while index < len(lines) and lines[index].strip() != GOAL_CLOSE:
            line_number = index + 1
            line = lines[index].strip()
            if not line or ":" not in line:
                raise ValueError(
                    f"goals.md:{line_number}: malformed metadata for `{title}`"
                )
            key, value = (part.strip() for part in line.split(":", 1))
            if key not in GOAL_KEYS:
                raise ValueError(
                    f"goals.md:{line_number}: unknown goal metadata key `{key}`"
                )
            if key in metadata:
                raise ValueError(
                    f"goals.md:{line_number}: duplicate goal metadata key `{key}`"
                )
            if not value:
                raise ValueError(
                    f"goals.md:{line_number}: empty goal metadata value `{key}`"
                )
            metadata[key] = value
            index += 1
        if index >= len(lines):
            raise ValueError(f"goals.md:{heading_line}: unclosed metadata for `{title}`")
        index += 1
        required = GOAL_KEYS - {"blocker-owner"}
        missing = required - metadata.keys()
        if missing:
            raise ValueError(f"goal `{title}` missing metadata: {sorted(missing)}")
        goal_id = metadata["id"]
        if goal_id in ids:
            raise ValueError(f"duplicate goal id: {goal_id}")
        ids.add(goal_id)
        status = metadata["status"]
        if status not in GOAL_STATUSES:
            raise ValueError(f"goal `{title}` has unknown status `{status}`")
        if status == "blocked" and not metadata.get("blocker-owner"):
            raise ValueError(f"blocked goal `{title}` lacks blocker-owner")
        goals.append(metadata)
    if not goals:
        raise ValueError("goals.md contains no structured goals")
    return goals


def workflow_summary(goals_text: str, todo_text: str) -> str:
    goals = parse_goal_metadata(goals_text)
    status_order = ("done", "in_progress", "todo", "proposed", "blocked")
    statuses = {status: 0 for status in status_order}
    for goal in goals:
        statuses[goal["status"]] += 1
    checked = sum(1 for line in todo_text.splitlines() if line.lstrip().startswith("- [x]"))
    open_items = sum(
        1 for line in todo_text.splitlines() if line.lstrip().startswith("- [ ]")
    )
    status_text = ", ".join(
        f"{statuses[status]} {status}" for status in status_order if statuses[status]
    )
    return (
        f"{len(goals)} goals ({status_text}) · "
        f"{checked} checked / {open_items} open todos"
    )


def relative_ledger_link(path: Path) -> str:
    if path.parent == ROOT:
        return "docs/claims.md"
    if path.parent == ROOT / "docs":
        return "claims.md"
    return "../docs/claims.md"


def render_region(
    mode: str,
    manifest: dict[str, Any],
    digest: str,
    document: Path,
    workflow: str,
) -> str:
    begin = f"<!-- BEGIN abbey-generated:claims-{mode} -->"
    end = f"<!-- END abbey-generated:claims-{mode} -->"
    source = "`src/claims.rs`"
    link = relative_ledger_link(document)
    lines = [begin, "<!-- Generated by tools/check_claims_sync.py; do not edit. -->"]
    lines.append(
        f"Canonical capability ledger: **{status_counts(manifest)}** "
        f"([full evidence]({link})). Source: {source}; schema 1; digest `{digest}`."
    )
    lines.append(
        f"Executable workflow ledger: **{workflow}** "
        "(stable goal metadata in `tasks/goals.md`)."
    )
    if mode == "table":
        lines.extend(
            [
                "",
                "| Stable ID | Status | Capability | Evidence boundary |",
                "| --- | --- | --- | --- |",
            ]
        )
        for claim in claim_rows(manifest):
            lines.append(
                "| `{}` | {} | {} | {} |".format(
                    markdown_cell(claim["id"]),
                    markdown_cell(status_label(claim["status"])),
                    markdown_cell(claim["name"]),
                    markdown_cell(claim["note"]),
                )
            )
    lines.append(end)
    return "\n".join(lines)


def render_evidence_document(
    manifest: dict[str, Any], digest: str, workflow: str
) -> str:
    lines = [
        "# Abbey capability evidence",
        "",
        "This file is generated from `src/claims.rs` by "
        "`tools/check_claims_sync.py --write`. Do not edit it by hand.",
        "",
        f"Schema: `1` · Digest: `{digest}` · {status_counts(manifest)}.",
        "",
        f"Workflow ledger: {workflow}. Goal evidence remains canonical in "
        "[`tasks/goals.md`](../tasks/goals.md).",
        "",
        "| Stable ID | Status | Capability | Implementation evidence | Automated tests | Local/live evidence | External evidence required | Next action | Blocker owner |",
        "| --- | --- | --- | --- | --- | --- | --- | --- | --- |",
    ]
    for claim in claim_rows(manifest):
        evidence = claim["evidence"]
        if not isinstance(evidence, dict):
            raise ValueError(f"claim {claim['id']} evidence must be an object")
        lines.append(
            "| `{}` | {} | {} | {} | {} | {} | {} | {} | {} |".format(
                markdown_cell(claim["id"]),
                markdown_cell(status_label(claim["status"])),
                markdown_cell(claim["name"]),
                markdown_cell(evidence.get("implementation_refs")),
                markdown_cell(evidence.get("automated_test_refs")),
                markdown_cell(evidence.get("local_live")),
                markdown_cell(evidence.get("external_required")),
                markdown_cell(claim["next_action"]),
                markdown_cell(claim["blocker_owner"]),
            )
        )
    return "\n".join(lines) + "\n"


def replace_generated_region(text: str, mode: str, replacement: str) -> str:
    begin = f"<!-- BEGIN abbey-generated:claims-{mode} -->"
    end = f"<!-- END abbey-generated:claims-{mode} -->"
    generated_begins = [
        line.strip() for line in text.splitlines() if line.strip().startswith(BEGIN_PREFIX)
    ]
    unknown = [marker for marker in generated_begins if marker != begin]
    if unknown:
        raise ValueError(f"unknown generated claims region: {unknown[0]}")
    if text.count(begin) > 1 or text.count(end) > 1:
        raise ValueError(f"duplicate generated claims-{mode} region")
    if (begin in text) != (end in text):
        raise ValueError(f"incomplete generated claims-{mode} region")
    if begin in text:
        prefix, rest = text.split(begin, 1)
        _, suffix = rest.split(end, 1)
        return prefix + replacement + suffix

    lines = text.splitlines()
    legacy = [index for index, line in enumerate(lines) if line.startswith(LEGACY_PREFIX)]
    if len(legacy) > 1:
        raise ValueError("multiple legacy claims digest markers")
    if legacy:
        insertion = legacy[0]
        lines.pop(insertion)
    else:
        insertion = 1 if lines and lines[0].startswith("#") else 0
    lines[insertion:insertion] = ["", replacement, ""]
    result = "\n".join(lines)
    return result + ("\n" if text.endswith("\n") and not result.endswith("\n") else "")


def atomic_write(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    descriptor, temporary = tempfile.mkstemp(prefix=f".{path.name}.", dir=path.parent)
    try:
        with os.fdopen(descriptor, "w", encoding="utf-8") as handle:
            handle.write(text)
            handle.flush()
            os.fsync(handle.fileno())
        os.replace(temporary, path)
    finally:
        if os.path.exists(temporary):
            os.unlink(temporary)


def synchronized_documents(
    manifest: dict[str, Any], digest: str
) -> dict[Path, str]:
    goals_text = (ROOT / "tasks/goals.md").read_text(encoding="utf-8")
    todo_text = (ROOT / "tasks/todo.md").read_text(encoding="utf-8")
    workflow = workflow_summary(goals_text, todo_text)
    rendered: dict[Path, str] = {}
    for path, mode in DOCUMENT_MODES.items():
        text = path.read_text(encoding="utf-8")
        if path == ROOT / "AGENTS.md" and "\n| Claim | Status | Evidence boundary |" in text:
            prefix, legacy = text.split("\n(source: `src/claims.rs`", 1)
            _, suffix = legacy.split("\n## Gotchas", 1)
            text = (
                prefix.rstrip()
                + "\n\nThe generated canonical table above is authoritative; "
                "`abbey claims manifest` exposes the same typed registry.\n\n## Gotchas"
                + suffix
            )
        region = render_region(mode, manifest, digest, path, workflow)
        rendered[path] = replace_generated_region(text, mode, region)
    rendered[GENERATED_LEDGER] = render_evidence_document(manifest, digest, workflow)
    return rendered


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--write",
        action="store_true",
        help="atomically refresh generated claim regions and the evidence ledger",
    )
    args = parser.parse_args()

    manifest, digest = canonical_manifest()
    failures: list[str] = []
    for path, expected in synchronized_documents(manifest, digest).items():
        actual = path.read_text(encoding="utf-8") if path.exists() else ""
        if args.write:
            if actual != expected:
                atomic_write(path, expected)
                print(f"updated {path.relative_to(ROOT)}")
        elif actual != expected:
            failures.append(str(path.relative_to(ROOT)))

    for path, forbidden_phrases in FORBIDDEN_DRIFT.items():
        text = path.read_text(encoding="utf-8")
        for phrase in forbidden_phrases:
            if phrase in text:
                failures.append(f"{path.relative_to(ROOT)} contains stale `{phrase}`")

    if failures:
        joined = ", ".join(failures)
        raise SystemExit(
            f"claims/docs synchronization is stale: {joined}; "
            "run python3 tools/check_claims_sync.py --write"
        )
    print(
        f"claims/docs: OK ({len(claim_rows(manifest))} claims, "
        f"schema {manifest['schema_version']}, sha256 {digest})"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
