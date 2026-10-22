"""Guards for the repository's cargo workspace topology.

The desktop client used to be a *second* cargo workspace with its own
lockfile. That meant the default `cargo build` / `cargo test` /
`cargo clippy` in this repository never built or tested the Tauri client: a
change to `src/app_core/` could break the client, and the default path would
not notice.

The client is now a member of the root workspace, and every member is a
*default* member — without `default-members`, a bare cargo command in a
workspace whose root is also a package selects only that root package. These
tests pin both facts, because either one can silently regress: a removed
`default-members` entry would quietly stop the default path from covering the
client, and a resurrected `desktop/Cargo.toml` would quietly reintroduce a
second lockfile that can drift from the root's.

Stdlib only: PyYAML and friends are not dependencies of this repo's tooling,
and check.sh runs these tests (via `unittest discover -s tools/tests`) on a
bare runner.
"""

from __future__ import annotations

from pathlib import Path
import re
import unittest

ROOT = Path(__file__).resolve().parents[2]
CARGO_TOML = ROOT / "Cargo.toml"
DESKTOP = ROOT / "desktop"


def manifest_tables() -> dict[str, str]:
    """The root manifest's top-level tables, as raw text."""
    tables: dict[str, str] = {}
    current = None
    lines: list[str] = []
    for raw in CARGO_TOML.read_text(encoding="utf-8").splitlines():
        header = re.match(r"^\[([^\]]+)\]$", raw.strip())
        if header:
            if current is not None:
                tables[current] = "\n".join(lines)
            current = header.group(1)
            lines = []
            continue
        if current is not None:
            lines.append(raw)
    if current is not None:
        tables[current] = "\n".join(lines)
    return tables


def listed_paths(table: str, key: str | None = None) -> list[str]:
    """The path strings in a TOML array.

    With `key`, the named array inside the table; without it, the first array
    the table contains.
    """
    if key is not None:
        match = re.search(rf'^\s*{re.escape(key)}\s*=\s*\[(.*?)\]', table, re.DOTALL | re.MULTILINE)
    else:
        match = re.search(r"\[(.*?)\]", table, re.DOTALL)
    if match is None:
        return []
    return re.findall(r'"([^"]+)"', match.group(1))


class WorkspaceTopologyTests(unittest.TestCase):
    def setUp(self) -> None:
        self.tables = manifest_tables()

    def test_the_root_manifest_declares_a_workspace(self) -> None:
        self.assertIn("workspace", self.tables)

    def test_the_desktop_client_is_a_workspace_member(self) -> None:
        members = listed_paths(self.tables["workspace"], "members")
        self.assertIn("desktop/codegen", members)
        self.assertIn("desktop/src-tauri", members)

    def test_every_member_is_also_a_default_member(self) -> None:
        # A bare `cargo build` in a workspace whose root is also a package
        # selects only that root package unless `default-members` says
        # otherwise. Without this the default path stops covering the client,
        # which is the whole point of the unification.
        members = set(listed_paths(self.tables["workspace"], "members"))
        defaults = set(listed_paths(self.tables["workspace"], "default-members"))
        self.assertTrue(defaults, "no default-members are declared")
        self.assertEqual(members - defaults, set())
        self.assertIn(".", defaults)

    def test_there_is_no_second_workspace_manifest(self) -> None:
        self.assertFalse(
            (DESKTOP / "Cargo.toml").exists(),
            "desktop/Cargo.toml would make the client a second workspace again",
        )

    def test_there_is_no_second_lockfile(self) -> None:
        self.assertFalse(
            (DESKTOP / "Cargo.lock").exists(),
            "a second lockfile can drift from the root's",
        )
        self.assertTrue((ROOT / "Cargo.lock").exists())

    def test_the_client_still_depends_on_the_library_and_not_the_reverse(self) -> None:
        # Membership must not turn the client into a parent of the CLI: the
        # desktop package depends on `abbey`, and the root package must not
        # depend on the desktop.
        client = (DESKTOP / "src-tauri" / "Cargo.toml").read_text(encoding="utf-8")
        self.assertRegex(client, r'abbey\s*=\s*\{\s*path\s*=\s*"\.\./\.\."')
        # Every dependency table of the root package is checked, not a prefix
        # of the file: the root manifest starts with the `[workspace]` table,
        # so a prefix split would find nothing at all and the assertion could
        # never fail.
        for name, table in self.tables.items():
            if not name.endswith("dependencies"):
                continue
            self.assertNotIn(
                "desktop/src-tauri",
                table,
                f"the root package must not depend on the client ({name})",
            )
            self.assertNotIn(
                "abbey-desktop",
                table,
                f"the root package must not depend on the client ({name})",
            )

    def test_the_client_keeps_its_own_lints(self) -> None:
        client = (DESKTOP / "src-tauri" / "Cargo.toml").read_text(encoding="utf-8")
        self.assertIn("unsafe_code", client)


if __name__ == "__main__":
    unittest.main()
