#!/usr/bin/env python3
"""Validate or refresh documentation stamps for Abbey's canonical claim ledger."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile


ROOT = Path(__file__).resolve().parent.parent
DOCUMENTS = (
    ROOT / "AGENTS.md",
    ROOT / "CLAUDE.md",
    ROOT / "README.md",
    ROOT / "tasks/goals.md",
    ROOT / "tasks/todo.md",
    ROOT / "tasks/lessons.md",
    ROOT / "docs/architecture.md",
    ROOT / "docs/identity.md",
    ROOT / "docs/production.md",
)
PREFIX = "<!-- abbey-claims-sha256: "
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
        "Abbey tool runtime\", \"—\", \"Out of scope",
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


def canonical_manifest() -> tuple[list[dict[str, object]], str]:
    result = subprocess.run(
        ["cargo", "run", "--quiet", "--bin", "abbey", "--", "claims", "manifest"],
        cwd=ROOT,
        check=True,
        capture_output=True,
        text=True,
        timeout=120,
    )
    manifest = json.loads(result.stdout)
    canonical = json.dumps(manifest, ensure_ascii=False, separators=(",", ":"))
    return manifest, hashlib.sha256(canonical.encode()).hexdigest()


def expected_marker(digest: str) -> str:
    return f"{PREFIX}{digest} -->"


def replace_marker(text: str, marker: str) -> str:
    lines = text.splitlines()
    matches = [index for index, line in enumerate(lines) if line.startswith(PREFIX)]
    if len(matches) > 1:
        raise ValueError("multiple claims digest markers")
    if matches:
        lines[matches[0]] = marker
    else:
        insertion = 1 if lines and lines[0].startswith("#") else 0
        lines.insert(insertion, marker)
    return "\n".join(lines) + ("\n" if text.endswith("\n") else "")


def atomic_write(path: Path, text: str) -> None:
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


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--write",
        action="store_true",
        help="atomically refresh documentation markers",
    )
    args = parser.parse_args()

    manifest, digest = canonical_manifest()
    marker = expected_marker(digest)
    failures: list[str] = []
    for path in DOCUMENTS:
        text = path.read_text(encoding="utf-8")
        refreshed = replace_marker(text, marker)
        if args.write:
            if refreshed != text:
                atomic_write(path, refreshed)
                print(f"updated {path.relative_to(ROOT)}")
        elif marker not in text.splitlines():
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
    print(f"claims/docs: OK ({len(manifest)} claims, sha256 {digest})")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
