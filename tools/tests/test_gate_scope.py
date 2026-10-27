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


def cohort_test_build_pairs(text: str) -> list[tuple[str, str | None]]:
    """Pair each cohort's `cargo test` command with the build that precedes it.

    Comments are not commands. A test run against another manifest (the
    vendored gtk compatibility crate) is out of scope. The build must sit in
    the same cohort block: the search stops at the previous `cargo test`, so
    one cohort's build can never vouch for the next cohort's test.
    """
    commands = []
    for raw in text.splitlines():
        line = raw.strip()
        if line and not line.startswith("#"):
            commands.append(line)
    pairs: list[tuple[str, str | None]] = []
    for index, line in enumerate(commands):
        if not re.match(r"^cargo\s+test\b", line) or "--manifest-path" in line:
            continue
        build = None
        for previous in reversed(commands[:index]):
            if re.match(r"^cargo\s+test\b", previous):
                break
            if re.match(r"^cargo\s+build\b", previous):
                build = previous
                break
        pairs.append((line, build))
    return pairs


def selection(line: str) -> tuple:
    """The (sorted `-p` packages, `--features` value) a cargo command selects."""
    packages = sorted(re.findall(r"(?:-p|--package)\s+(\S+)", line))
    features = re.search(r"--features[\s=](\S+)", line)
    return (tuple(packages), features.group(1) if features else None)


def assert_feature_matched(text: str) -> None:
    """Fail unless every cohort test is preceded by its own feature-matched bin build."""
    for test, build in cohort_test_build_pairs(text):
        if build is None:
            raise AssertionError(f"no cohort build precedes: {test}")
        if "--bins" not in build or "--locked" not in build:
            raise AssertionError(f"cohort build is not a locked bin build: {build}")
        if selection(build) != selection(test):
            raise AssertionError(f"build {build!r} does not match test {test!r}")


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


class CohortBinaryBuildTests(unittest.TestCase):
    """Each feature cohort must build its own binaries before it tests.

    Integration tests spawn the `abbey`/`abbeyd` executables from the target
    directory. If a cohort tested without first building its bins with the
    same package and feature selection, it would exercise whatever binary an
    earlier cohort left behind -- a default-feature binary under a `wdbx` or
    `accel` test run -- and pass against the wrong artifact.
    """

    def test_every_cohort_builds_its_own_bins_before_testing(self) -> None:
        text = CHECK_SH.read_text(encoding="utf-8")
        pairs = cohort_test_build_pairs(text)
        self.assertGreaterEqual(len(pairs), 4, "check.sh lost a feature cohort")
        for test, build in pairs:
            with self.subTest(test=test):
                self.assertIsNotNone(build, "cohort tests without building its bins")
                assert build is not None
                self.assertIn("--bins", build)
                self.assertIn("--locked", build)
                self.assertEqual(selection(build), selection(test))
        assert_feature_matched(text)

    def test_a_cohort_test_without_its_own_build_is_rejected(self) -> None:
        script = "\n".join(
            [
                "cargo build --locked --bins",
                "cargo test --locked",
                "cargo clippy --locked -p abbey --features wdbx --all-targets -- -D warnings",
                "cargo test --locked -p abbey --features wdbx",
            ]
        )
        with self.assertRaises(AssertionError):
            assert_feature_matched(script)

    def test_a_build_for_a_different_feature_set_is_rejected(self) -> None:
        script = "\n".join(
            [
                "cargo build --locked -p abbey --features wdbx --bins",
                "cargo test --locked -p abbey --features accel",
            ]
        )
        with self.assertRaises(AssertionError):
            assert_feature_matched(script)

    def test_vendor_manifest_tests_are_out_of_scope(self) -> None:
        script = "\n".join(
            [
                "# cargo test --locked -p abbey --features accel",
                "cargo test --locked --manifest-path vendor/Cargo.toml -p gtk-compat-tests",
                "cargo build --locked --bins",
                "cargo test --locked",
            ]
        )
        pairs = cohort_test_build_pairs(script)
        self.assertEqual(pairs, [("cargo test --locked", "cargo build --locked --bins")])


if __name__ == "__main__":
    unittest.main()
