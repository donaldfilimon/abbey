#!/usr/bin/env python3
"""Fail closed if Program 3 acquires write, transport, store, or content code.

The boundary covers the closed synthetic guild-intelligence surfaces and any
future live-capture module (`guild_live_capture`), which is held to the same
rules until its contract (docs/program-3-live-capture-contract.md) has been
independently reviewed and authorised.
"""

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SURFACES = (
    ROOT / "src/app_core/guild_intelligence.rs",
    ROOT / "src/app_core/guild_intelligence/plan.rs",
    ROOT / "src/app_core/guild_intelligence/validation.rs",
)
# Live-capture module paths. There is no live-capture module today; these
# patterns make the checker cover one the moment it appears. The full RULES
# apply to them, including the network rule. That is deliberately fail-closed:
# any network allowance for an approved read-only adapter belongs to the later
# reviewed adapter task after OB-18 (independent review, manager authorisation,
# participant consent), not to this checker.
LIVE_CAPTURE_PATTERNS = (
    "src/app_core/guild_live_capture.rs",
    "src/app_core/guild_live_capture/**/*.rs",
    "src/guild_live_capture.rs",
    "src/guild_live_capture/**/*.rs",
)
RULES = (
    (r"\b(?:reqwest|ureq|hyper|serenity)::", "network client"),
    (r"\b(?:std|tokio)::net\b", "network client"),
    (r"\b(?:std|tokio)::process\b|\b(?:Command|Child)::", "process execution"),
    # `std::fs` alone misses a brace import (`use std::{fs};`), where the
    # effect is reached as `fs::write(...)`. The path segment is matched on
    # its own so the bound does not depend on how the module was imported.
    (r"\bstd::fs\b|\bfs::|\b(?:File|OpenOptions)::", "filesystem access"),
    (r"\b(?:abi_wdbx|rusqlite|RuntimeStore|GuildRegistry)\b", "durable store"),
    (r"\b(?:runtime_v3|ToolRoute|ToolDescriptor|register_tool)\b", "runtime tool"),
    (r"\b(?:Executor|Actuator|ApprovalRequest|Compensation)\b", "Program 5 type"),
    (
        r"\bfn\s+(?:execute|apply|approve|compensate|create|edit|delete|send|write|persist)_",
        "effect operation",
    ),
    (
        r"\b(?:message_content|message_body|transcript|audio_bytes|credential|access_token)\b",
        "private content",
    ),
)


def _code_only(text: str) -> str:
    text = re.sub(r"/\*.*?\*/", "", text, flags=re.DOTALL)
    text = re.sub(r"//.*", "", text)
    return re.sub(r'"(?:\\.|[^"\\])*"', '""', text)


def live_capture_surfaces(root=ROOT):
    root = Path(root)
    found = set()
    for pattern in LIVE_CAPTURE_PATTERNS:
        found.update(path for path in root.glob(pattern) if path.is_file())
    return sorted(found)


def all_surfaces(root=ROOT):
    root = Path(root)
    program_three = [root / path.relative_to(ROOT) for path in SURFACES]
    if root != ROOT:
        program_three = [path for path in program_three if path.is_file()]
    return program_three + live_capture_surfaces(root)


def scan_paths(paths):
    findings = []
    for path in paths:
        code = _code_only(Path(path).read_text(encoding="utf-8"))
        for pattern, label in RULES:
            if re.search(pattern, code):
                findings.append(f"{Path(path)}: forbidden {label}")
    return findings


def main() -> int:
    findings = scan_paths(all_surfaces())
    if findings:
        print("\n".join(findings), file=sys.stderr)
        return 1
    print("Program 3 read-only boundary: OK")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
