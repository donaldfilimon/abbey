#!/usr/bin/env python3
"""Fail closed if Program 3 acquires write, transport, store, or content code."""

from pathlib import Path
import re
import sys


ROOT = Path(__file__).resolve().parents[1]
SURFACES = (
    ROOT / "src/app_core/guild_intelligence.rs",
    ROOT / "src/app_core/guild_intelligence/validation.rs",
)
RULES = (
    (r"\b(?:reqwest|ureq|hyper|serenity)::", "network client"),
    (r"\b(?:std|tokio)::net\b", "network client"),
    (r"\b(?:std|tokio)::process\b|\b(?:Command|Child)::", "process execution"),
    (r"\bstd::fs\b|\b(?:File|OpenOptions)::", "filesystem access"),
    (r"\b(?:abi_wdbx|rusqlite|RuntimeStore|GuildRegistry)\b", "durable store"),
    (r"\b(?:runtime_v3|ToolRoute|ToolDescriptor|register_tool)\b", "runtime tool"),
    (r"\b(?:Executor|Actuator|ApprovalRequest|Compensation)\b", "Program 5 type"),
    (r"\bfn\s+(?:execute|apply|approve|compensate|create|edit|delete|send|write|persist)_", "effect operation"),
    (r"\b(?:message_content|message_body|transcript|audio_bytes|credential|access_token)\b", "private content"),
)


def _code_only(text: str) -> str:
    text = re.sub(r"/\*.*?\*/", "", text, flags=re.DOTALL)
    text = re.sub(r"//.*", "", text)
    return re.sub(r'"(?:\\.|[^"\\])*"', '""', text)


def scan_paths(paths):
    findings = []
    for path in paths:
        code = _code_only(Path(path).read_text(encoding="utf-8"))
        for pattern, label in RULES:
            if re.search(pattern, code):
                findings.append(f"{Path(path)}: forbidden {label}")
    return findings


def main() -> int:
    findings = scan_paths(SURFACES)
    if findings:
        print("\n".join(findings), file=sys.stderr)
        return 1
    print("Program 3 read-only boundary: OK")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
