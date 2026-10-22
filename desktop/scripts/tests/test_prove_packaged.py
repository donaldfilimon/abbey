"""Static proof checks for the packaged Routes prover. Never opens a window.

The packaged prover drives the binary inside a real `.app` bundle, so it
needs a signed bundle, a window server, and Accessibility permission. This
suite asserts the parts that can be checked without any of them.

The property that matters most here is *which bundle* the prover drives:
both editions stage into the same directory, so a name-order pick is a
silent wrong-edition proof. The suite therefore pins the selection to the
bundle's own identifier and proves that the old alphabetical pick would
have chosen the wrong one.
"""

import contextlib
import io
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

SCRIPTS = Path(__file__).resolve().parents[1]
DESKTOP = SCRIPTS.parent
sys.path.insert(0, str(SCRIPTS))

import prove_packaged_macos as prover


SAFE_IDENTIFIER = "com.donaldfilimon.abbey"
PERSONAL_IDENTIFIER = "com.donaldfilimon.abbey-personal"


def bundle(root, name, identifier):
    """A fixture bundle with a real Info.plist and the expected executable."""
    path = Path(root) / name
    (path / "Contents/MacOS").mkdir(parents=True)
    (path / "Contents/MacOS/abbey-desktop").write_bytes(b"")
    (path / "Contents/Info.plist").write_text(
        '<?xml version="1.0" encoding="UTF-8"?>\n'
        '<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" '
        '"http://www.apple.com/DTDs/PropertyList-1.0.dtd">\n'
        '<plist version="1.0"><dict>'
        f"<key>CFBundleIdentifier</key><string>{identifier}</string>"
        "</dict></plist>\n"
    )
    return path


class DeclaredExpectations(unittest.TestCase):
    """The edition table is explicit and is not weakened silently."""

    def test_editions_are_bound_to_the_bundle_identifier(self):
        self.assertEqual(prover.EDITIONS_BY_IDENTIFIER, {
            SAFE_IDENTIFIER: "safe",
            PERSONAL_IDENTIFIER: "personal",
        })

    def test_the_default_expectation_is_the_shipped_safe_edition(self):
        self.assertEqual(prover.DEFAULT_EDITION, "safe")
        self.assertEqual(prover.EDITIONS_BY_IDENTIFIER[SAFE_IDENTIFIER],
                         prover.DEFAULT_EDITION)

    def test_the_prover_source_has_no_name_order_bundle_pick(self):
        text = Path(prover.__file__).read_text()
        # The old pick was `sorted(glob("*.app"))[-1]`. The glob still exists
        # (the staged directory is scanned), but no *code* takes the last
        # entry of a listing: the selection goes through the identifier.
        code = text.split('"""', 2)[2]
        self.assertNotIn("[-1]", code)
        self.assertNotIn("candidates[-1]", code)
        self.assertIn("select_bundle(", code)
        self.assertIn("--expect-edition", text)
        self.assertIn("bundle_edition_mismatch", text)
        self.assertIn("no_staged_bundle_for_expected_edition", text)


class BundleSelection(unittest.TestCase):
    """The bundle under test is chosen by identifier, never by name order."""

    def staged(self, root):
        bundle(root, "Abbey.app", SAFE_IDENTIFIER)
        bundle(root, "Abbey Personal.app", PERSONAL_IDENTIFIER)

    def test_the_safe_edition_is_selected_by_its_identifier(self):
        with tempfile.TemporaryDirectory() as root:
            self.staged(root)
            chosen = prover.select_bundle(root, "safe")
            self.assertEqual(chosen["path"].name, "Abbey.app")
            self.assertEqual(chosen["edition"], "safe")
            self.assertEqual(chosen["identifier"], SAFE_IDENTIFIER)

    def test_the_personal_edition_is_selected_by_its_identifier(self):
        with tempfile.TemporaryDirectory() as root:
            self.staged(root)
            chosen = prover.select_bundle(root, "personal")
            self.assertEqual(chosen["path"].name, "Abbey Personal.app")
            self.assertEqual(chosen["edition"], "personal")

    def test_the_old_alphabetical_pick_followed_whatever_happened_to_be_staged(self):
        # This is the trap the identifier selection closes. The old pick was
        # `sorted(glob("*.app"))[-1]`: it followed the directory listing, not
        # any expectation. With only the personal bundle staged it proved the
        # *personal* bundle while the gate believed it proved the shipped
        # one, and the receipt recorded only a path — never an edition.
        with tempfile.TemporaryDirectory() as root:
            bundle(root, "Abbey Personal.app", PERSONAL_IDENTIFIER)
            alphabetical = sorted(Path(root).glob("*.app"))[-1]
            self.assertEqual(alphabetical.name, "Abbey Personal.app")
            # The expectation decides now: the safe edition is not staged, so
            # the run refuses rather than proving some other edition.
            with self.assertRaisesRegex(
                    prover.ProofFailure,
                    "no_staged_bundle_for_expected_edition"):
                prover.select_bundle(root, "safe")
            # And the personal bundle is reachable only when it is asked for.
            self.assertEqual(prover.select_bundle(root, "personal")["path"].name,
                             "Abbey Personal.app")

    def test_both_editions_staged_still_selects_by_identifier(self):
        with tempfile.TemporaryDirectory() as root:
            self.staged(root)
            self.assertEqual(prover.select_bundle(root, "safe")["path"].name,
                             "Abbey.app")
            self.assertEqual(prover.select_bundle(root, "personal")["path"].name,
                             "Abbey Personal.app")

    def test_a_bundle_with_an_unknown_identifier_is_never_selected(self):
        with tempfile.TemporaryDirectory() as root:
            bundle(root, "Somebody Else.app", "com.example.somebodyelse")
            for edition in ("safe", "personal"):
                with self.assertRaises(prover.ProofFailure):
                    prover.select_bundle(root, edition)

    def test_an_unreadable_plist_is_not_an_edition(self):
        with tempfile.TemporaryDirectory() as root:
            broken = Path(root) / "Broken.app"
            (broken / "Contents").mkdir(parents=True)
            (broken / "Contents/Info.plist").write_text("not a plist")
            self.assertEqual(prover.bundle_identifier(broken), "")
            self.assertIsNone(
                prover.staged_bundles(root)[0]["edition"])

    def test_no_bundle_for_the_expected_edition_fails_loudly(self):
        with tempfile.TemporaryDirectory() as root:
            bundle(root, "Abbey Personal.app", PERSONAL_IDENTIFIER)
            with self.assertRaisesRegex(prover.ProofFailure,
                                        "no_staged_bundle_for_expected_edition"):
                prover.select_bundle(root, "safe")

    def test_two_bundles_of_the_same_edition_are_ambiguous(self):
        with tempfile.TemporaryDirectory() as root:
            bundle(root, "Abbey.app", SAFE_IDENTIFIER)
            bundle(root, "Abbey Copy.app", SAFE_IDENTIFIER)
            with self.assertRaisesRegex(
                    prover.ProofFailure,
                    "ambiguous_staged_bundle_for_expected_edition"):
                prover.select_bundle(root, "safe")

    def test_an_unknown_expected_edition_is_refused(self):
        with tempfile.TemporaryDirectory() as root:
            with self.assertRaisesRegex(prover.ProofFailure,
                                        "unknown_expected_edition"):
                prover.select_bundle(root, "enterprise")


class RunnerArguments(unittest.TestCase):
    """The runner enforces the expectation instead of guessing."""

    def run_main(self, argv):
        with tempfile.TemporaryDirectory() as root:
            root = Path(root)
            receipt_path = root / "packaged-receipt.json"
            original = sys.argv
            sys.argv = ["prove_packaged_macos.py", *argv]
            try:
                with patch.object(prover.base, "OUT", root), \
                        contextlib.redirect_stdout(io.StringIO()), \
                        contextlib.redirect_stderr(io.StringIO()):
                    code = prover.main()
            finally:
                sys.argv = original
            receipt = json.loads(receipt_path.read_text())
            return code, receipt

    def test_an_explicit_bundle_of_the_wrong_edition_is_refused(self):
        with tempfile.TemporaryDirectory() as root:
            personal = bundle(root, "Abbey Personal.app", PERSONAL_IDENTIFIER)
            code, receipt = self.run_main(["--app", str(personal)])
            self.assertEqual(code, 1)
            self.assertEqual(receipt["failure"], "bundle_edition_mismatch")

    def test_an_explicit_bundle_of_the_expected_edition_is_accepted(self):
        with tempfile.TemporaryDirectory() as root:
            safe = bundle(root, "Abbey.app", SAFE_IDENTIFIER)
            code, receipt = self.run_main(["--app", str(safe)])
            # The bundle is accepted; the run then fails later (no signature,
            # no Accessibility) — never with an edition failure.
            self.assertEqual(code, 1)
            self.assertNotEqual(receipt["failure"], "bundle_edition_mismatch")
            self.assertEqual(receipt["bundle"]["edition"], "safe")
            self.assertEqual(receipt["bundle"]["identifier"], SAFE_IDENTIFIER)
            self.assertEqual(receipt["bundle"]["expected_edition"], "safe")

    def test_the_receipt_records_the_edition_that_was_proved(self):
        with tempfile.TemporaryDirectory() as root:
            safe = bundle(root, "Abbey.app", SAFE_IDENTIFIER)
            _, receipt = self.run_main(["--app", str(safe),
                                        "--expect-edition", "safe"])
            self.assertEqual(Path(receipt["bundle"]["path"]).name, "Abbey.app")
            self.assertEqual(receipt["bundle"]["edition"], "safe")

    def test_an_unknown_argument_is_refused(self):
        code, receipt = self.run_main(["--not-an-argument"])
        self.assertEqual(code, 1)
        self.assertEqual(receipt["failure"], "unknown_argument")

    def test_an_unknown_expected_edition_is_refused(self):
        code, receipt = self.run_main(["--expect-edition", "enterprise"])
        self.assertEqual(code, 1)
        self.assertEqual(receipt["failure"], "unknown_expected_edition")


class HarnessShape(unittest.TestCase):
    """The wrapper and the gate wiring are part of the proof."""

    def test_the_wrapper_is_a_thin_shell_that_forwards_its_arguments(self):
        wrapper = (SCRIPTS / "prove-packaged-macos.sh").read_text()
        self.assertIn("set -eu", wrapper)
        self.assertIn('exec python3 scripts/prove_packaged_macos.py "$@"', wrapper)

    def test_the_gate_states_the_edition_it_proves(self):
        gate = (DESKTOP / "check.sh").read_text()
        self.assertIn("--expect-edition", gate)
        self.assertIn('${ABBEY_DESKTOP_PACKAGED_EDITION:-safe}', gate)

    def test_the_menu_prover_uses_the_same_identifier_table(self):
        import prove_menu_icon_macos as menu_prover
        self.assertEqual(
            {identifier: entry["edition"]
             for identifier, entry in menu_prover.EDITIONS_BY_IDENTIFIER.items()},
            prover.EDITIONS_BY_IDENTIFIER,
        )


if __name__ == "__main__":
    unittest.main()
