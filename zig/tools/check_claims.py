#!/usr/bin/env python3
"""Claims sync: docs/claims.md must equal `abbey-zig claims --markdown`, and
every test a Current row names must exist (a `test "<name>"` in src/, or a
`stage "<name>"` in tools/check.sh for `gate:` names)."""
import re
import subprocess
import sys
from pathlib import Path

root = Path(__file__).resolve().parent.parent
binary = root / "zig-out" / "bin" / "abbey-zig"
generated = subprocess.run([str(binary), "claims", "--markdown"], check=True, capture_output=True).stdout.decode()
doc = root / "docs" / "claims.md"
if "--write" in sys.argv:
    doc.write_text(generated)
    print(f"wrote {doc}")
current = doc.read_text() if doc.exists() else ""
if current != generated:
    print("FAIL: docs/claims.md drifted from `abbey-zig claims --markdown` (run tools/check_claims.py --write)")
    sys.exit(1)

tests = set()
for f in (root / "src").rglob("*.zig"):
    tests.update(re.findall(r'^test "((?:[^"\\]|\\.)*)"', f.read_text(), re.M))
stages = set(re.findall(r'^stage "([^"]+)"', (root / "tools" / "check.sh").read_text(), re.M))

bad = 0
rows = 0
for line in generated.splitlines():
    cells = [c.strip() for c in line.split(" | ")]
    if len(cells) < 5 or cells[1] != "Current":
        continue
    rows += 1
    names = re.findall(r"`([^`]+)`", cells[4])
    if not names:
        print(f"FAIL: Current row {cells[0]} names no tests"); bad += 1
    for n in names:
        if n.startswith("gate:"):
            if not any(s == n[5:] or s.startswith(n[5:]) for s in stages):
                print(f"FAIL: {cells[0]}: gate stage {n!r} not in tools/check.sh"); bad += 1
        elif n not in tests:
            print(f"FAIL: {cells[0]}: test {n!r} not found in src/"); bad += 1
if bad:
    sys.exit(1)
print(f"claims ok: {rows} Current rows, every named test exists")
