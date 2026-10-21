"""Drive the updater feed assembler over an owned fixture release.

Only the shell script's bytes are exercised, through /bin/sh with a
system-tools-only environment; no live release, key, or network is involved.

The signatures under fixtures/ were produced by the installed Tauri CLI
(`tauri signer generate`, then `tauri signer sign --app-version ...`), except
`signed-no-file.sig`, which is hand-built: the CLI always records the file it
signed, so no real signature can lack that field, and the assembler's refusal
of one is exactly what that fixture proves. See fixtures/README.md.
"""

from __future__ import annotations

import base64
import json
from pathlib import Path
import subprocess
import tempfile
import unittest


SCRIPT = Path(__file__).resolve().parents[1] / "assemble-update-feed.sh"
FIXTURES = Path(__file__).resolve().parent / "fixtures"

ARTIFACT = "Abbey.app.tar.gz"
PERSONAL_ARTIFACT = "Abbey Personal.app.tar.gz"
URL = (
    "https://github.com/donaldfilimon/abbey/releases/download/"
    "abbey-desktop-v0.1.0/Abbey.app.tar.gz"
)

# The exact bytes every fixture signature was made over, so a fixture is a
# real signature over the artifact the tests publish.
PAYLOAD = b"abbey desktop updater feed fixture payload\n"


def fixture(name: str) -> str:
    """A fixture signature's bytes, whitespace-stripped.

    The Tauri CLI writes its .sig without a trailing newline; the repository
    keeps one, and the assembler strips whitespace before publishing the
    signature because the plugin's base64 decoder rejects whitespace.
    """
    return (FIXTURES / name).read_text(encoding="utf-8").strip()


SIGNED_0_1_0 = fixture("signed-0.1.0-Abbey.app.tar.gz.sig")
SIGNED_2_6_0 = fixture("signed-2.6.0-Abbey.app.tar.gz.sig")
SIGNED_2_6_0_PERSONAL = fixture("signed-2.6.0-Abbey-Personal.app.tar.gz.sig")
SIGNED_NO_VERSION = fixture("unversioned-Abbey.app.tar.gz.sig")
SIGNED_NO_FILE = fixture("signed-no-file.sig")


class AssembleUpdateFeed(unittest.TestCase):
    """One fake edition's artifacts in, one v2 feed JSON out — fail closed."""

    def run_feed(self, root, *, artifact=ARTIFACT, sig="Abbey.app.tar.gz.sig",
                 sig_text=SIGNED_0_1_0, version="0.1.0", extra=()):
        """Create fixture files under root and run the assembler on them.

        sig=None omits the .sig file entirely; sig_text replaces its
        contents; version=None leaves --version out of the command line;
        extra appends command-line arguments the run never asked for. The
        environment is restricted so the script cannot reach anything except
        the fixture paths it was given.
        """
        if artifact is not None:
            artifact_path = root / artifact
            artifact_path.parent.mkdir(parents=True, exist_ok=True)
            artifact_path.write_bytes(PAYLOAD)
        if sig is not None:
            sig_path = root / sig
            sig_path.parent.mkdir(parents=True, exist_ok=True)
            sig_path.write_text(sig_text)
        command = [
            "--artifact", str(root / (artifact if artifact is not None else "missing.tar.gz")),
            "--sig", str(root / (sig if sig is not None else "missing.sig")),
            "--url", URL,
        ]
        if version is not None:
            command.extend(["--version", version])
        command.extend([
            "--platform", "darwin-aarch64",
            "--out", str(root / "latest.json"),
        ])
        command.extend(extra)
        return subprocess.run(
            ["/bin/sh", str(SCRIPT), *command],
            capture_output=True, text=True, timeout=15,
            env={"PATH": "/usr/bin:/bin", "HOME": str(root)},
        )

    def test_writes_a_valid_v2_feed_for_the_fixture_release(self):
        with tempfile.TemporaryDirectory(prefix="abbey-feed fixture-", dir="/tmp") as temporary:
            root = Path(temporary)
            process = self.run_feed(root)
            self.assertEqual(process.returncode, 0, process.stderr)
            feed = json.loads((root / "latest.json").read_text())
            self.assertEqual(feed["version"], "0.1.0")
            self.assertEqual(sorted(feed), ["platforms", "pub_date", "version"])
            self.assertEqual(sorted(feed["platforms"]), ["darwin-aarch64"])
            entry = feed["platforms"]["darwin-aarch64"]
            # Exactly the two fields the plugin reads: `url` and `signature`.
            # The signed artifact name is checked by the assembler but NOT
            # published, so a future `deny_unknown_fields` on the plugin's
            # platform struct cannot reject an already-published feed.
            self.assertEqual(sorted(entry), ["signature", "url"])
            self.assertEqual(entry["url"], URL)
            # The signature is the byte-exact, whitespace-stripped .sig
            # contents — never re-encoded and never taken from the artifact.
            self.assertEqual(entry["signature"], SIGNED_0_1_0)
            self.assertRegex(feed["pub_date"], r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z$")
            self.assertTrue((root / "latest.json").read_text().endswith("\n"))
            self.assertIn("assemble-update-feed: wrote", process.stdout)
            self.assertIn(str(root / "latest.json"), process.stdout)
            self.assertIn("darwin-aarch64", process.stdout)

    def test_feed_includes_notes_only_when_asked(self):
        with tempfile.TemporaryDirectory(prefix="abbey-feed fixture-", dir="/tmp") as temporary:
            root = Path(temporary)
            process = self.run_feed(root, extra=("--notes", "safe edition 0.1.0"))
            self.assertEqual(process.returncode, 0, process.stderr)
            feed = json.loads((root / "latest.json").read_text())
            self.assertIn("notes", feed)
            self.assertEqual(feed["notes"], "safe edition 0.1.0")

    def test_missing_artifact_fails_closed(self):
        with tempfile.TemporaryDirectory(prefix="abbey-feed fixture-", dir="/tmp") as temporary:
            root = Path(temporary)
            process = self.run_feed(root, artifact=None)
            self.assertNotEqual(process.returncode, 0)
            self.assertIn("artifact is not a file", process.stderr)
            self.assertFalse((root / "latest.json").exists())

    def test_missing_signature_file_fails_closed(self):
        with tempfile.TemporaryDirectory(prefix="abbey-feed fixture-", dir="/tmp") as temporary:
            root = Path(temporary)
            process = self.run_feed(root, sig=None)
            self.assertNotEqual(process.returncode, 0)
            self.assertIn("signature is not a file", process.stderr)
            self.assertFalse((root / "latest.json").exists())

    def test_empty_signature_fails_closed(self):
        with tempfile.TemporaryDirectory(prefix="abbey-feed fixture-", dir="/tmp") as temporary:
            root = Path(temporary)
            process = self.run_feed(root, sig_text="")
            self.assertNotEqual(process.returncode, 0)
            self.assertFalse((root / "latest.json").exists())

    def test_multiline_signature_fails_closed(self):
        # A wrapped .sig would still base64-decode after stripping, but the
        # bundler emits one line: anything else is not a Tauri signature.
        with tempfile.TemporaryDirectory(prefix="abbey-feed fixture-", dir="/tmp") as temporary:
            root = Path(temporary)
            process = self.run_feed(root, sig_text="dGVzdA==\ndGVzdA==\n")
            self.assertNotEqual(process.returncode, 0)
            self.assertIn("one non-blank base64 line", process.stderr)
            self.assertFalse((root / "latest.json").exists())

    def test_non_base64_signature_fails_closed(self):
        with tempfile.TemporaryDirectory(prefix="abbey-feed fixture-", dir="/tmp") as temporary:
            root = Path(temporary)
            process = self.run_feed(root, sig_text="not base64 at all !!")
            self.assertNotEqual(process.returncode, 0)
            self.assertIn("not valid base64", process.stderr)
            self.assertFalse((root / "latest.json").exists())

    def test_non_https_url_fails_closed(self):
        with tempfile.TemporaryDirectory(prefix="abbey-feed fixture-", dir="/tmp") as temporary:
            root = Path(temporary)
            process = self.run_feed(root, extra=("--url", "http://example.test/Abbey.app.tar.gz"))
            self.assertNotEqual(process.returncode, 0)
            self.assertFalse((root / "latest.json").exists())

    def test_unknown_argument_fails_closed(self):
        # No positional text and no invented flag: the assembler only knows
        # the documented options.
        for extra in (("unrequested",), ("--sig-sha256", "deadbeef")):
            with self.subTest(extra=extra):
                with tempfile.TemporaryDirectory(prefix="abbey-feed fixture-", dir="/tmp") as temporary:
                    root = Path(temporary)
                    process = self.run_feed(root, extra=extra)
                    self.assertNotEqual(process.returncode, 0)
                    self.assertIn("unknown argument", process.stderr)
                    self.assertFalse((root / "latest.json").exists())

    def test_unknown_platform_fails_closed(self):
        with tempfile.TemporaryDirectory(prefix="abbey-feed fixture-", dir="/tmp") as temporary:
            root = Path(temporary)
            process = self.run_feed(root, extra=("--platform", "darwin-x86_64"))
            self.assertNotEqual(process.returncode, 0)
            self.assertFalse((root / "latest.json").exists())

    def test_bad_semver_fails_closed(self):
        with tempfile.TemporaryDirectory(prefix="abbey-feed fixture-", dir="/tmp") as temporary:
            root = Path(temporary)
            process = self.run_feed(root, extra=("--version", "0.1"))
            self.assertNotEqual(process.returncode, 0)
            self.assertIn("semver", process.stderr)
            self.assertFalse((root / "latest.json").exists())

    def test_feed_is_bound_to_the_signed_version_and_file(self):
        # The feed announces what the signature covers: the version inside the
        # trusted comment and the file name next to it.
        with tempfile.TemporaryDirectory(prefix="abbey-feed fixture-", dir="/tmp") as temporary:
            root = Path(temporary)
            process = self.run_feed(root, sig_text=SIGNED_2_6_0, version="2.6.0")
            self.assertEqual(process.returncode, 0, process.stderr)
            feed = json.loads((root / "latest.json").read_text())
            self.assertEqual(feed["version"], "2.6.0")
            entry = feed["platforms"]["darwin-aarch64"]
            self.assertEqual(entry["signature"], SIGNED_2_6_0)
            self.assertNotIn("signed_file", entry)

    def test_version_is_derived_from_the_signature_when_not_given(self):
        with tempfile.TemporaryDirectory(prefix="abbey-feed fixture-", dir="/tmp") as temporary:
            root = Path(temporary)
            process = self.run_feed(root, sig_text=SIGNED_2_6_0, version=None)
            self.assertEqual(process.returncode, 0, process.stderr)
            feed = json.loads((root / "latest.json").read_text())
            self.assertEqual(feed["version"], "2.6.0")

    def test_personal_edition_asset_name_is_accepted(self):
        # The personal edition's artifact name contains a space, which is the
        # name the release workflow publishes.
        with tempfile.TemporaryDirectory(prefix="abbey-feed fixture-", dir="/tmp") as temporary:
            root = Path(temporary)
            process = self.run_feed(
                root,
                artifact=PERSONAL_ARTIFACT,
                sig="Abbey Personal.app.tar.gz.sig",
                sig_text=SIGNED_2_6_0_PERSONAL,
                version="2.6.0",
            )
            self.assertEqual(process.returncode, 0, process.stderr)
            feed = json.loads((root / "latest.json").read_text())
            self.assertEqual(feed["version"], "2.6.0")
            entry = feed["platforms"]["darwin-aarch64"]
            self.assertEqual(entry["signature"], SIGNED_2_6_0_PERSONAL)
            self.assertNotIn("signed_file", entry)

    def test_version_mismatch_fails_closed(self):
        # The anti-downgrade guard: the feed may not announce a version the
        # signature does not cover.
        with tempfile.TemporaryDirectory(prefix="abbey-feed fixture-", dir="/tmp") as temporary:
            root = Path(temporary)
            process = self.run_feed(root, sig_text=SIGNED_2_6_0, version="2.6.1")
            self.assertNotEqual(process.returncode, 0)
            self.assertIn("version mismatch", process.stderr)
            self.assertIn("2.6.1", process.stderr)
            self.assertIn("2.6.0", process.stderr)
            self.assertFalse((root / "latest.json").exists())

    def test_signature_without_a_version_fails_closed(self):
        # requireSignedVersion is enabled: a signature that records no version
        # cannot be published as a feed.
        with tempfile.TemporaryDirectory(prefix="abbey-feed fixture-", dir="/tmp") as temporary:
            root = Path(temporary)
            process = self.run_feed(root, sig_text=SIGNED_NO_VERSION, version=None)
            self.assertNotEqual(process.returncode, 0)
            self.assertIn("no version", process.stderr)
            self.assertIn("requireSignedVersion", process.stderr)
            self.assertFalse((root / "latest.json").exists())

    def test_signature_without_a_file_name_fails_closed(self):
        with tempfile.TemporaryDirectory(prefix="abbey-feed fixture-", dir="/tmp") as temporary:
            root = Path(temporary)
            process = self.run_feed(root, sig_text=SIGNED_NO_FILE, version=None)
            self.assertNotEqual(process.returncode, 0)
            self.assertIn("no file:", process.stderr)
            self.assertFalse((root / "latest.json").exists())

    def test_signed_file_name_mismatch_fails_closed(self):
        # The cross-edition mixup guard: a signature made for the safe
        # edition's asset must never be published for the personal one.
        with tempfile.TemporaryDirectory(prefix="abbey-feed fixture-", dir="/tmp") as temporary:
            root = Path(temporary)
            process = self.run_feed(
                root,
                artifact=PERSONAL_ARTIFACT,
                sig_text=SIGNED_2_6_0,
                version="2.6.0",
            )
            self.assertNotEqual(process.returncode, 0)
            self.assertIn("signed artifact name mismatch", process.stderr)
            self.assertIn(ARTIFACT, process.stderr)
            self.assertIn(PERSONAL_ARTIFACT, process.stderr)
            self.assertFalse((root / "latest.json").exists())

    def test_signature_without_a_trusted_comment_fails_closed(self):
        # Base64 that decodes to something that is not a minisign signature.
        with tempfile.TemporaryDirectory(prefix="abbey-feed fixture-", dir="/tmp") as temporary:
            root = Path(temporary)
            process = self.run_feed(root, sig_text=base64.b64encode(b"test").decode())
            self.assertNotEqual(process.returncode, 0)
            self.assertIn("trusted comment", process.stderr)
            self.assertFalse((root / "latest.json").exists())


if __name__ == "__main__":
    unittest.main()
