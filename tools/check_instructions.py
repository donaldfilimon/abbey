#!/usr/bin/env python3
"""Keep the agent-instruction files from drifting apart.

`AGENTS.md` is canonical. `CLAUDE.md` is a pointer that may hold only the
generated claims summary, the pointer paragraph, the command table, the
`src/` module map, and the machine git-policy block. This check fails when:

- `CLAUDE.md` gains any other heading or prose (rules must live in AGENTS.md);
- the machine git-policy block differs between the two files;
- a module named in the `CLAUDE.md` module map, or a repository path named in
  its command table, no longer exists;
- an editor-side instruction surface appears that is not a short pointer to
  AGENTS.md.
"""

from __future__ import annotations

from pathlib import Path
import re
import sys


ROOT = Path(__file__).resolve().parents[1]
AGENTS = ROOT / "AGENTS.md"
CLAUDE = ROOT / "CLAUDE.md"

POINTER = """See [AGENTS.md](AGENTS.md) — canonical. This file keeps only the command table
and the `src/` module map; every rule, trap, and gate description lives in
AGENTS.md, and the execution-path diagram and docs map live in
[docs/architecture.md](docs/architecture.md). `tools/check_instructions.py`
(a `./check.sh` step) fails if other content reappears here."""

CLAUDE_HEADINGS = (
    "# CLAUDE.md",
    "## Commands",
    "## Module map (`src/`)",
    "## Git workflow (machine policy, 2026-08-27)",
)
SUMMARY_END = "<!-- END abbey-generated:claims-summary -->"
GIT_OPEN = "<!-- machine-git-policy -->"
GIT_CLOSE = "<!-- /machine-git-policy -->"

# Editor-side surfaces other tools load. None exists today; if one appears it
# must be a short pointer to AGENTS.md rather than a second rule set.
POINTER_SURFACES = (
    "GEMINI.md",
    ".github/copilot-instructions.md",
    ".cursorrules",
    ".windsurfrules",
)
POINTER_SURFACE_DIRS = (".cursor/rules", ".codex", ".agents")
POINTER_MAX_LINES = 8


def git_policy_block(text: str, name: str) -> tuple[str | None, list[str]]:
    if text.count(GIT_OPEN) != 1 or text.count(GIT_CLOSE) != 1:
        return None, [f"{name}: expected exactly one {GIT_OPEN} … {GIT_CLOSE} block"]
    start = text.index(GIT_OPEN)
    end = text.index(GIT_CLOSE) + len(GIT_CLOSE)
    if end <= start:
        return None, [f"{name}: {GIT_CLOSE} precedes {GIT_OPEN}"]
    return text[start:end], []


def check_claude_shape(text: str, root: Path = ROOT) -> list[str]:
    findings: list[str] = []
    headings = tuple(line for line in text.splitlines() if line.startswith("#"))
    if headings != CLAUDE_HEADINGS:
        findings.append(
            f"CLAUDE.md headings must be exactly {list(CLAUDE_HEADINGS)}; found {list(headings)}"
        )
    if text.count(SUMMARY_END) != 1:
        findings.append("CLAUDE.md: generated claims-summary block missing or duplicated")
        return findings
    between = text.split(SUMMARY_END, 1)[1]
    between = between.split("\n## Commands\n", 1)[0].strip("\n")
    if between != POINTER:
        findings.append("CLAUDE.md: prose between the claims summary and ## Commands must be exactly the pointer paragraph")
    if "## Commands" in text and "## Module map (`src/`)" in text:
        commands = text.split("\n## Commands\n", 1)[1].split("\n## Module map (`src/`)\n", 1)[0].strip("\n")
        if not (commands.startswith("```bash\n") and commands.endswith("\n```")) or commands.count("```") != 2:
            findings.append("CLAUDE.md: ## Commands must contain exactly one ```bash block and nothing else")
        module_map = text.split("\n## Module map (`src/`)\n", 1)[1].split(GIT_OPEN, 1)[0].strip("\n")
        if not all(line.startswith("|") for line in module_map.splitlines()):
            findings.append("CLAUDE.md: ## Module map (`src/`) must contain only the module table")
        findings.extend(check_module_map(module_map, root / "src"))
        findings.extend(check_command_paths(commands, root))
    return findings


def _first_cell(line: str) -> str:
    cells = re.split(r"(?<!\\)\|", line)
    return cells[1] if len(cells) > 2 else ""


def check_module_map(table: str, src: Path = ROOT / "src") -> list[str]:
    findings: list[str] = []
    for line in table.splitlines()[2:]:
        base = src
        for token in re.findall(r"`([^`]+)`", _first_cell(line)):
            if not re.fullmatch(r"[A-Za-z0-9_./]+", token):
                continue
            candidate = src / token
            if not candidate.exists() and (base / token).exists():
                candidate = base / token
            if not candidate.exists():
                findings.append(f"CLAUDE.md module map names src/{token}, which does not exist")
                continue
            if candidate.is_dir():
                base = candidate
    return findings


def check_command_paths(commands: str, root: Path = ROOT) -> list[str]:
    findings: list[str] = []
    for token in sorted(set(re.findall(r"(?<![\w./-])((?:\./|tools/|scripts/)[\w./-]+\.(?:sh|py))", commands))):
        if not (root / token).is_file():
            findings.append(f"CLAUDE.md command table names {token}, which does not exist")
    return findings


def check_pointer_surfaces(root: Path = ROOT) -> list[str]:
    findings: list[str] = []
    paths = [root / name for name in POINTER_SURFACES]
    for directory in POINTER_SURFACE_DIRS:
        if (root / directory).is_dir():
            paths.extend(p for p in sorted((root / directory).rglob("*")) if p.is_file())
    for path in paths:
        if not path.is_file():
            continue
        text = path.read_text(encoding="utf-8", errors="replace")
        lines = [line for line in text.splitlines() if line.strip()]
        rel = path.relative_to(root)
        if "AGENTS.md" not in text or len(lines) > POINTER_MAX_LINES:
            findings.append(
                f"{rel}: instruction surfaces other than AGENTS.md/CLAUDE.md must be a pointer to AGENTS.md of at most {POINTER_MAX_LINES} lines"
            )
    return findings


def check(root: Path = ROOT) -> list[str]:
    agents = (root / "AGENTS.md").read_text(encoding="utf-8")
    claude = (root / "CLAUDE.md").read_text(encoding="utf-8")
    findings = check_claude_shape(claude, root)
    agents_git, errors = git_policy_block(agents, "AGENTS.md")
    findings.extend(errors)
    claude_git, errors = git_policy_block(claude, "CLAUDE.md")
    findings.extend(errors)
    if agents_git is not None and claude_git is not None and agents_git != claude_git:
        findings.append("machine git-policy block differs between AGENTS.md and CLAUDE.md")
    findings.extend(check_pointer_surfaces(root))
    return findings


def main() -> int:
    findings = check()
    for finding in findings:
        print(f"FAIL {finding}", file=sys.stderr)
    if findings:
        return 1
    print("instructions: OK (AGENTS.md canonical; CLAUDE.md pointer, command table, module map)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
