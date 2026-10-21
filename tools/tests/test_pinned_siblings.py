"""Guards for the pinned-sibling verification helper.

`tools/ci/verify-pinned-siblings.sh` reproduces the gate CI runs by
overriding Abbey's sibling *path dependencies* to clean archives of the
revisions `.github/workflows/rust.yml` pins. It exists because the local
`../abi` and `../wdbx` checkouts carry their own in-flight work: a sibling
that is mid-change can fail Abbey's gate for reasons that have nothing to do
with Abbey, and the sibling's sources are not ours to reformat or adjust.

Two properties make the helper trustworthy, and this file pins both:

- the pins are *read* from the workflow rather than repeated here, so the
  helper cannot drift from what CI builds against;
- the helper never writes into a sibling checkout, so the owner's in-flight
  sibling work is never disturbed.

Stdlib only: PyYAML and friends are not dependencies of this repo's tooling,
and check.sh runs these tests (via `unittest discover -s tools/tests`) on a
bare runner.
"""

from __future__ import annotations

from pathlib import Path
import re
import subprocess
import unittest

ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "tools" / "ci" / "verify-pinned-siblings.sh"
WORKFLOW = ROOT / ".github" / "workflows" / "rust.yml"

REVISION = re.compile(r"\b[0-9a-f]{40}\b")

# The helper addresses the sibling checkouts through `"$root/../$sibling"`,
# so a write-guard has to recognise *that* shape and not only a literal
# `../abi`. The helper's own temporary files (`"$workspace/$sibling.tar"`) are
# not sibling checkouts and must not match.
SIBLING_PATH = (
    r"(?:\.\./(?:abi|wdbx)"
    r"|\$root/\.\./\$sibling"
    r"|\$\{root\}/\.\./\$\{sibling\})"
)
# Git's writing subcommands. The lookbehind keeps a revision suffix such as
# `^{commit}` from reading as the `commit` subcommand.
WRITE_VERBS = (
    r"(?<![\w^{])(checkout|reset|clean|apply|commit|stash|add|rm|mv|push|init)\b"
)
# Commands that write, paired with a sibling path.
WRITE_PATTERNS = (
    # A writing command taking a sibling path as an argument.
    rf"\b(cp|mv|rsync|rm|rmdir|ln|mkdir|touch|chmod|chown)\s+[^\n]*?{SIBLING_PATH}",
    # A git subcommand that writes, aimed at a sibling checkout.
    rf"git\s+-C\s+[^\n]*?{SIBLING_PATH}[^\n]*?{WRITE_VERBS}",
    rf"git\s+-C\s+[^\n]*?{WRITE_VERBS}[^\n]*?{SIBLING_PATH}",
    # An archive extracted over a sibling checkout.
    rf"\btar\s+[^\n]*?(-C|--directory)\s+[\"']?[^\n]*?{SIBLING_PATH}",
    # A redirect into a sibling checkout.
    rf">>?\s*[\"']?[^\n]*?{SIBLING_PATH}",
)


def workflow_pins() -> dict[str, str]:
    """The sibling revisions the workflow pins."""
    text = WORKFLOW.read_text(encoding="utf-8")
    pins = {}
    for name in ("ABI_REVISION", "WDBX_REVISION"):
        match = re.search(rf"^\s+{name}:\s*(\S+)\s*$", text, re.MULTILINE)
        if match is None:
            raise AssertionError(f"the workflow pins no {name}")
        pins[name] = match.group(1)
    return pins


def script_text() -> str:
    return SCRIPT.read_text(encoding="utf-8")


class PinnedSiblingHelperTests(unittest.TestCase):
    def test_the_helper_exists_and_is_valid_shell(self) -> None:
        self.assertTrue(SCRIPT.is_file())
        result = subprocess.run(
            ["sh", "-n", str(SCRIPT)], capture_output=True, text=True
        )
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_the_helper_repeats_no_revision_of_its_own(self) -> None:
        # A hardcoded revision would outlive the pin it was copied from.
        for revision in REVISION.findall(script_text()):
            with self.subTest(revision=revision):
                self.fail(f"the helper hardcodes a revision: {revision}")

    def test_the_helper_reads_the_pins_from_the_workflow(self) -> None:
        text = script_text()
        self.assertIn("ABI_REVISION", text)
        self.assertIn("WDBX_REVISION", text)
        self.assertIn(".github/workflows/rust.yml", text)

    def test_the_helper_uses_exactly_the_workflow_pins(self) -> None:
        result = subprocess.run(
            ["sh", str(SCRIPT), "--print-pins"], capture_output=True, text=True
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        reported = dict(
            line.split("=", 1) for line in result.stdout.splitlines() if "=" in line
        )
        self.assertEqual(reported, workflow_pins())

    def test_the_helper_never_writes_into_a_sibling_checkout(self) -> None:
        # The sibling checkouts hold another session's in-flight work. Every
        # write has to land in the helper's own temporary workspace, so no
        # command may take a sibling path as a write target — in the shape the
        # helper actually uses, not only in a literal `../abi` spelling.
        text = script_text()
        for pattern in WRITE_PATTERNS:
            with self.subTest(pattern=pattern):
                self.assertIsNone(re.search(pattern, text))

    def test_the_helper_reads_the_siblings_without_a_writing_git_subcommand(self) -> None:
        text = script_text()
        # The only git verbs aimed at a sibling are the read-only ones. The
        # subcommand is the token after the path and any `-c key=value`
        # options; a revision suffix like `^{commit}` is not a subcommand.
        pattern = re.compile(r"git\s+-C\s+\S+(?:\s+-c\s+\S+)*\s+([a-z-]+)")
        seen = 0
        for match in pattern.finditer(text):
            seen += 1
            subcommand = match.group(1)
            with self.subTest(subcommand=subcommand):
                self.assertIsNone(
                    re.fullmatch(r"checkout|reset|clean|apply|commit|stash|add|rm|mv|push|init", subcommand),
                    f"a writing git subcommand targets a sibling: {subcommand}",
                )
        self.assertTrue(seen, "the helper runs no git command against a sibling")

    def test_the_helper_cleans_up_its_workspace_on_a_signal(self) -> None:
        # A handler that only cleans up lets the script continue into the gate
        # with the pinned override already deleted.
        text = script_text()
        self.assertIn("trap cleanup EXIT", text)
        self.assertRegex(text, r"trap 'cleanup; exit \d+' INT")
        self.assertRegex(text, r"trap 'cleanup; exit \d+' TERM")

    def test_the_helper_keeps_the_build_out_of_a_shared_target_directory(self) -> None:
        text = script_text()
        self.assertIn("CARGO_TARGET_DIR", text)
        # The isolated directory is the default; a caller-supplied one must be
        # an explicit opt-in rather than an accident.
        self.assertIn("--shared-target", text)


if __name__ == "__main__":
    unittest.main()
