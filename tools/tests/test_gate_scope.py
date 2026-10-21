"""Guards for the scope of the root gate's rustfmt step.

`cargo fmt --all` does not only mean "every crate in this workspace". Cargo
resolves the local *path dependencies* as well, so in this repository the
sibling `../abi` and `../wdbx` checkouts are formatted too. That couples
Abbey's merge bar to two other repositories:

- a sibling's in-flight formatting (or its rustfmt style edition) fails
  `./check.sh` for a change that never touched Abbey, and
- a plain `cargo fmt` run inside Abbey rewrites the sibling's sources.

The siblings have their own gates. The root gate must judge Abbey only, so
the rustfmt step has to select Abbey's package explicitly — and it must keep
selecting *all* of that package's targets, because the integration tests in
`tests/` are targets of the `abbey` package and a narrower selection would
silently stop formatting them.

Stdlib only: PyYAML and friends are not dependencies of this repo's tooling,
and check.sh runs these tests (via `unittest discover -s tools/tests`) on a
bare runner.
"""

from __future__ import annotations

from pathlib import Path
import re
import unittest

ROOT = Path(__file__).resolve().parents[2]
CHECK_SH = ROOT / "check.sh"
CARGO_TOML = ROOT / "Cargo.toml"

# Cargo flags that would narrow the formatted target set below the package's
# own targets. `--lib` would drop the integration tests in `tests/`.
NARROWING_FLAGS = ("--lib", "--bin", "--bins", "--example", "--examples", "--test")


def package_names() -> list[str]:
    """The package names the root manifest declares."""
    text = CARGO_TOML.read_text(encoding="utf-8")
    match = re.search(r"^\[package\]\s*$(.*?)(?=^\[)", text, re.MULTILINE | re.DOTALL)
    if match is None:
        raise AssertionError("the root manifest declares no [package] table")
    name = re.search(r'^name\s*=\s*"([^"]+)"', match.group(1), re.MULTILINE)
    if name is None:
        raise AssertionError("the root manifest declares no package name")
    return [name.group(1)]


def fmt_invocations() -> list[str]:
    """Every `cargo fmt` *command* in the root gate (comments are not commands)."""
    invocations = []
    for raw in CHECK_SH.read_text(encoding="utf-8").splitlines():
        line = raw.strip()
        if line.startswith("#"):
            continue
        if re.match(r"^cargo\s+fmt\b", line):
            invocations.append(line)
    return invocations


class GateScopeTests(unittest.TestCase):
    def test_the_gate_has_a_rustfmt_step(self) -> None:
        self.assertTrue(fmt_invocations(), "check.sh runs no cargo fmt step")

    def test_rustfmt_is_package_scoped(self) -> None:
        # `--all` reaches the sibling path dependencies; `--workspace` does the
        # same. Neither may come back to the root gate.
        for invocation in fmt_invocations():
            with self.subTest(invocation=invocation):
                self.assertNotIn("--all", invocation)
                self.assertNotIn("--workspace", invocation)
                self.assertIn("--package", invocation)

    def test_rustfmt_still_covers_every_target_of_the_package(self) -> None:
        # Selecting the package must not narrow it further: `--lib` would
        # stop formatting the integration tests in `tests/`.
        for invocation in fmt_invocations():
            for flag in NARROWING_FLAGS:
                with self.subTest(invocation=invocation, flag=flag):
                    self.assertNotIn(flag, invocation)

    def test_every_selected_package_is_a_real_workspace_member(self) -> None:
        members = set(package_names())
        selected: set[str] = set()
        for invocation in fmt_invocations():
            for name in re.findall(r"--package\s+(\S+)", invocation):
                selected.add(name)
        self.assertTrue(selected, "the rustfmt step selects no package")
        self.assertEqual(selected - members, set())

    def test_the_rustfmt_step_checks_rather_than_rewrites(self) -> None:
        for invocation in fmt_invocations():
            with self.subTest(invocation=invocation):
                self.assertIn("--check", invocation)


if __name__ == "__main__":
    unittest.main()
