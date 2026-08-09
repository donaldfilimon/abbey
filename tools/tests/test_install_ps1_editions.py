"""Parser guards for the Windows installer's edition separation.

`src/edition.rs` is the single identity source for both compiled editions, and
`install.sh` already derives its installed names from the built binary rather
than repeating a literal. `install.ps1` must do the same, or a personal-edition
build on Windows overwrites the safe edition's binary and completion.

These are *parser* tests, deliberately: no PowerShell runtime exists on the
development machine, so the script cannot be executed here. They pin the
script's source-level derivation and the concrete filenames that derivation
produces for each edition — they do not prove Windows packaging works.

Stdlib only: PyYAML and friends are not dependencies of this repo's tooling,
and check.sh runs these tests (via `unittest discover -s tools/tests`) on a
bare runner.
"""

from __future__ import annotations

from pathlib import Path
import re
import unittest

ROOT = Path(__file__).resolve().parents[2]
INSTALL_PS1 = ROOT / "install.ps1"
INSTALL_SH = ROOT / "install.sh"
EDITION_RS = ROOT / "src" / "edition.rs"

# The names a shipped Windows install already uses. Hardcoded on purpose: the
# derivation below is checked *against* these, so a change to src/edition.rs
# that would re-point an existing install fails here instead of shipping.
SAFE_INSTALLED_BINARY = "abbey.exe"
SAFE_INSTALLED_COMPLETION = "_abbey.ps1"
SAFE_DAEMON_BINARY = "abbeyd.exe"

PERSONAL_INSTALLED_BINARY = "abbey-personal.exe"
PERSONAL_INSTALLED_COMPLETION = "_abbey-personal.ps1"
PERSONAL_DAEMON_BINARY = "abbey-personal-daemon.exe"


def identity_table(text: str, const_name: str) -> dict[str, str]:
    """Return the string fields of one `EditionIdentity` table in edition.rs."""
    match = re.search(
        rf"^const {const_name}: EditionIdentity = EditionIdentity \{{\n(.*?)^\}};",
        text,
        re.DOTALL | re.MULTILINE,
    )
    if match is None:
        raise AssertionError(f"src/edition.rs has no `{const_name}` identity table")
    fields = dict(re.findall(r'^\s*(\w+):\s*"([^"]*)",\s*$', match.group(1), re.MULTILINE))
    if not fields:
        raise AssertionError(f"`{const_name}` parsed to no fields — regex drifted")
    return fields


def name_template(text: str, variable: str) -> str:
    """Return the double-quoted template assigned to `$<variable>` once."""
    matches = re.findall(
        rf'^\${re.escape(variable)}\s*=\s*"([^"]*)"\s*$', text, re.MULTILINE
    )
    if len(matches) != 1:
        raise AssertionError(
            f"install.ps1 must assign ${variable} exactly once (found {len(matches)})"
        )
    return matches[0]


def expand(template: str, values: dict[str, str]) -> str:
    """Evaluate a `$(...)`-only PowerShell interpolation template."""
    rendered = template
    for variable, value in values.items():
        rendered = rendered.replace(f"$({variable})", value)
    if "$" in rendered:
        raise AssertionError(
            f"template {template!r} references a name this test cannot resolve"
        )
    return rendered


class InstallPs1Source(unittest.TestCase):
    def setUp(self) -> None:
        self.text = INSTALL_PS1.read_text(encoding="utf-8")

    def test_no_literal_installed_names_survive(self) -> None:
        # These are the four occurrences that made a personal-edition install
        # clobber the safe one. `abbey` as a *directory* component and the
        # `.abbey-` staging prefix are fine — only the installed filenames are
        # forbidden as literals.
        for literal in ("abbey.exe", "_abbey.ps1"):
            self.assertNotIn(
                literal,
                self.text,
                f"install.ps1 still hardcodes the safe edition's {literal}",
            )

    def test_build_output_path_uses_the_cargo_bin_name_not_the_edition_name(self) -> None:
        # Both editions compile the same `[[bin]] name = "abbey"` target, so the
        # release artifact path is edition-independent. Deriving it from the
        # edition would break the personal install with "missing release binary".
        self.assertRegex(self.text, r'(?m)^\$cargoBinName\s*=\s*"abbey"\s*$')
        bin_line = re.search(r"^\$bin\s*=\s*Join-Path.*$", self.text, re.MULTILINE)
        self.assertIsNotNone(bin_line, "install.ps1 no longer computes $bin")
        assert bin_line is not None
        self.assertIn("$($cargoBinName)", bin_line.group(0))
        self.assertNotIn("$($editionBin)", bin_line.group(0))

    def test_edition_names_are_probed_from_the_built_binary(self) -> None:
        # Same identity source and same subcommand install.sh uses, so the two
        # installers can never grow a second naming scheme.
        self.assertRegex(
            self.text,
            r"(?m)^\$editionBin\s*=\s*\(& \$bin edition --name \|.*\)\.Trim\(\)\s*$",
        )
        self.assertRegex(
            self.text,
            r"(?m)^\$editionDaemon\s*=\s*\(& \$bin edition --daemon-name \|.*\)\.Trim\(\)\s*$",
        )
        sh = INSTALL_SH.read_text(encoding="utf-8")
        self.assertIn("edition --name", sh)
        self.assertIn("edition --daemon-name", sh)

    def test_a_failed_edition_probe_throws_instead_of_falling_back(self) -> None:
        # A fallback to a literal name is exactly the collision being closed:
        # a personal build whose probe failed would install as the safe binary.
        for variable in ("editionBin", "editionDaemon"):
            self.assertRegex(
                self.text,
                rf"if \(\$LASTEXITCODE -ne 0 -or -not \${variable}\) \{{\n\s*throw ",
                f"${variable} needs a throwing guard, not a fallback literal",
            )

    def test_feature_selection_matches_install_sh(self) -> None:
        sh = INSTALL_SH.read_text(encoding="utf-8")
        self.assertIn("ABBEY_CARGO_FEATURES", sh)
        self.assertIn("$env:ABBEY_CARGO_FEATURES", self.text)
        self.assertRegex(
            self.text,
            r"cargo build --release --locked --features \$env:ABBEY_CARGO_FEATURES",
        )
        self.assertRegex(self.text, r"(?m)^\s*cargo build --release --locked\s*$")

    def test_no_daemon_binary_is_installed_on_windows(self) -> None:
        # The authenticated daemon is Unix-socket-only; install.ps1 has never
        # installed it and must not start now. The derived name is reported,
        # never written.
        self.assertIn('Write-Host "not installed: $daemonFileName', self.text)
        for line in self.text.splitlines():
            if "$daemonFileName" in line:
                self.assertNotRegex(line, r"(Copy-Item|Move-Item|New-Item|Out-File)")


class InstalledNamesPerEdition(unittest.TestCase):
    """The derivation, evaluated for both editions against edition.rs."""

    def setUp(self) -> None:
        self.text = INSTALL_PS1.read_text(encoding="utf-8")
        edition_rs = EDITION_RS.read_text(encoding="utf-8")
        self.safe = identity_table(edition_rs, "SAFE")
        self.personal = identity_table(edition_rs, "PERSONAL")
        self.templates = {
            key: name_template(self.text, key)
            for key in ("binFileName", "daemonFileName", "completionFileName")
        }

    def names_for(self, identity: dict[str, str]) -> dict[str, str]:
        values = {
            "$editionBin": identity["binary_name"],
            "$editionDaemon": identity["daemon_binary_name"],
        }
        return {key: expand(tpl, values) for key, tpl in self.templates.items()}

    def test_safe_edition_names_are_pinned_to_todays_values(self) -> None:
        names = self.names_for(self.safe)
        self.assertEqual(names["binFileName"], SAFE_INSTALLED_BINARY)
        self.assertEqual(names["completionFileName"], SAFE_INSTALLED_COMPLETION)
        self.assertEqual(names["daemonFileName"], SAFE_DAEMON_BINARY)

    def test_personal_edition_names_are_pinned(self) -> None:
        names = self.names_for(self.personal)
        self.assertEqual(names["binFileName"], PERSONAL_INSTALLED_BINARY)
        self.assertEqual(names["completionFileName"], PERSONAL_INSTALLED_COMPLETION)
        self.assertEqual(names["daemonFileName"], PERSONAL_DAEMON_BINARY)

    def test_every_installed_filename_differs_between_editions(self) -> None:
        safe = self.names_for(self.safe)
        personal = self.names_for(self.personal)
        for key in self.templates:
            self.assertNotEqual(
                safe[key], personal[key], f"editions collide on {key} in install.ps1"
            )

    def test_the_templates_actually_reference_the_derived_names(self) -> None:
        # Guards against a template that pins a literal and therefore passes the
        # equality checks above for the safe edition while ignoring the edition.
        self.assertIn("$(", self.templates["binFileName"])
        self.assertIn("$(", self.templates["daemonFileName"])
        self.assertIn("$(", self.templates["completionFileName"])
        self.assertNotIn("abbey", self.templates["binFileName"])
        self.assertNotIn("abbey", self.templates["daemonFileName"])
        self.assertNotIn("abbey", self.templates["completionFileName"])


if __name__ == "__main__":
    unittest.main()
