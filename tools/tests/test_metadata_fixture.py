"""Owned metadata fixture must not spend its production deadline on site hooks."""
from pathlib import Path
import subprocess
import sys
import tempfile
import time
from types import SimpleNamespace
import unittest
from unittest.mock import patch

TESTS = Path(__file__).resolve().parent
sys.path.insert(0, str(TESTS))


class CancellationOrdering(unittest.TestCase):
    def test_draft_enter_precedes_readiness_and_escape_follows_queue_observation(self):
        import smoke_tui_metadata_pty as metadata
        interactions = []
        expected = (b"\x1b[200~/doctor \x1b[201~\r"
                    b"\x1b[200~metadata-queued-never-contact\x1b[201~\r")
        record = dict(pid=11, leaf_pid=12, ppid=13)

        class Session:
            proc = SimpleNamespace(pid=10)

            def __init__(self, *_):
                pass

            def submit(self, text):
                self.send(b"\x1b[200~" + text.encode() + b"\x1b[201~\r")

            def send(inner, data):
                if data == b"\x1b":
                    self.assertIn("leaf", interactions)
                    self.assertIn("exact", interactions)
                    self.assertIn("parent-live", interactions)
                    self.assertIn(b"queued (1)", interactions)
                interactions.append(data)

            def wait_output(self, data, **_):
                interactions.append(data)

            def wait_join(inner, code):
                self.assertEqual(code, 130)
                interactions.append("joined")

            def quit_observed(inner):
                self.assertIn("closed", interactions)
                interactions.append("quit")

        class Fixture:
            def __init__(self, *_):
                pass

            def set_control(self, *_, **__):
                pass

            def wait_call(inner, *_):
                self.assertEqual(interactions, [expected],
                                 "both Enter inputs must be sent in one write before readiness")
                interactions.append("call")
                return record

            def assert_exact_probe(self, _):
                interactions.append("exact")

            def wait_leaf(inner, *_):
                self.assertIn(expected, interactions)
                interactions.append("leaf")

            def assert_closed(inner, *_, **__):
                self.assertIn("joined", interactions)
                self.assertIn(b"discarded 1 queued prompt", interactions)
                interactions.append("closed")

            def events(self):
                interactions.append("no-generation")
                return []

            def cleanup(inner, _):
                if "closed" in interactions:
                    self.assertIn("no-generation", interactions)
                interactions.append("cleanup")

        def exists(pid):
            self.assertEqual(pid, record["ppid"])
            interactions.append("parent-live")
            return True

        with patch.object(metadata, "MetadataFixture", Fixture), \
                patch.object(metadata.harness, "OwnedSession", Session), \
                patch.object(metadata.harness, "exists", exists):
            metadata.cancellation(Path("unused"), Path("unused"))
        self.assertEqual(interactions[-1], "cleanup")


class MetadataFixture(unittest.TestCase):
    def test_generation_forwards_original_arguments(self):
        import smoke_tui_metadata_pty as metadata
        metadata.harness.OPTIONS = SimpleNamespace(state_env="ABBEY_STATE_DIR", config_env="ABBEY_CONFIG")
        with tempfile.TemporaryDirectory(prefix="abbey-native-forward-") as scratch:
            fixture = metadata.MetadataFixture(Path(scratch))
            args = ["complete", "--prompt", "metadata-queued-never-contact", "two words"]
            result = subprocess.run([str(fixture.root / "bin/abi"), *args], env=fixture.env,
                                    capture_output=True, timeout=4, check=True)
            call, = fixture.events(fixture.nonce)
            self.assertEqual(call["argv"], args)
            self.assertEqual(call["backend"], "abi")
            self.assertEqual(call["action"], "generation")
            self.assertIn(fixture.marker.encode(), result.stdout)
            self.assertEqual(subprocess.check_output([str(fixture.root / "bin/abi"), "--version"],
                                                    env=fixture.env), b"owned-fixture-version\n")

    def test_compiler_failure_is_hard_failure(self):
        import os
        import smoke_tui_metadata_pty as metadata
        metadata.harness.OPTIONS = SimpleNamespace(state_env="ABBEY_STATE_DIR", config_env="ABBEY_CONFIG")
        with tempfile.TemporaryDirectory(prefix="abbey-native-compiler-") as scratch:
            with patch.dict(os.environ, {"CC": "/usr/bin/false"}):
                with self.assertRaisesRegex(AssertionError, "compiler failed"):
                    metadata.MetadataFixture(Path(scratch))
        with tempfile.TemporaryDirectory(prefix="abbey-native-missing-") as scratch:
            with patch.dict(os.environ, {"CC": str(Path(scratch) / "missing-cc")}):
                with self.assertRaises(FileNotFoundError):
                    metadata.MetadataFixture(Path(scratch))

    def test_sidecar_rejects_unbounded_or_injected_marker(self):
        import smoke_tui_metadata_pty as metadata
        metadata.harness.OPTIONS = SimpleNamespace(state_env="ABBEY_STATE_DIR", config_env="ABBEY_CONFIG")
        with tempfile.TemporaryDirectory(prefix="abbey-native-bounds-") as scratch:
            fixture = metadata.MetadataFixture(Path(scratch))
            original = (fixture.root / "native-control").read_bytes()
            for marker in ("x" * 256, "injected\nfield", "invalid space"):
                with self.assertRaises(AssertionError):
                    fixture.set_control("metadata", marker=marker)
                self.assertEqual((fixture.root / "native-control").read_bytes(), original)

    def test_version_does_not_invoke_configured_python(self):
        import os
        import smoke_tui_metadata_pty as metadata
        metadata.harness.OPTIONS = SimpleNamespace(state_env="ABBEY_STATE_DIR", config_env="ABBEY_CONFIG")
        with tempfile.TemporaryDirectory(prefix="abbey-native-poison-") as scratch:
            root = Path(scratch)
            poison = root / "poison"
            marker = root / "poison-ran"
            poison.write_text("#!/bin/sh\nprintf poison > '" + str(marker) + "'\nexit 71\n")
            poison.chmod(0o700)
            with patch.object(sys, "executable", str(poison)):
                fixture = metadata.MetadataFixture(root)
            fixture.set_control("metadata", hold=True, marker="owned")
            proc = subprocess.Popen([str(root / "bin/abi"), "--version"], env=fixture.env,
                                    start_new_session=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            try:
                deadline = time.monotonic() + 2
                while time.monotonic() < deadline and not fixture.events(fixture.nonce, "leaf"):
                    time.sleep(.01)
                self.assertFalse(marker.exists())
                call, = fixture.events(fixture.nonce)
                leaf, = fixture.events(fixture.nonce, "leaf")
                fixture.assert_exact_probe(call)
                self.assertEqual(call["leaf_pid"], leaf["pid"])
                self.assertEqual(leaf["ppid"], call["pid"])
                self.assertEqual(call["pgid"], leaf["pgid"])
                self.assertTrue(metadata.harness.exists(call["pid"]))
                self.assertTrue(metadata.harness.exists(leaf["pid"]))
                proc.terminate()
                proc.communicate(timeout=3)
                self.assertEqual(proc.returncode, 130)
                self.assertFalse(metadata.harness.exists(leaf["pid"]))
                self.assertFalse(metadata.harness.exists(call["pgid"], True))
                self.assertFalse((root / "release").exists())
            finally:
                if proc.poll() is None:
                    proc.terminate()
                proc.communicate(timeout=4)

    @unittest.skipUnless(sys.platform != "win32", "Unix PTY fixture")
    def test_metadata_leaf_skips_unrelated_python_site_hooks(self):
        import smoke_tui_metadata_pty as metadata
        metadata.harness.OPTIONS = SimpleNamespace(state_env="ABBEY_STATE_DIR", config_env="ABBEY_CONFIG")
        with tempfile.TemporaryDirectory(prefix="abbey-metadata-sites-") as scratch:
            root = Path(scratch)
            fixture = metadata.MetadataFixture(root)
            hooks = root / "hooks"
            hooks.mkdir()
            marker = root / "site-hook-ran"
            (hooks / "sitecustomize.py").write_text(
                "import time\nfrom pathlib import Path\n"
                f"Path({str(marker)!r}).write_text('unexpected')\ntime.sleep(2)\n")
            fixture.env["PYTHONPATH"] = str(hooks)
            fixture.set_control("metadata", hold=True, marker="owned")
            proc = subprocess.Popen([str(root / "bin/abi"), "--version"], env=fixture.env,
                                    stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            try:
                deadline = time.monotonic() + 1
                while time.monotonic() < deadline and not fixture.events(fixture.nonce, "leaf"):
                    time.sleep(.01)
                self.assertFalse(marker.exists(), "the metadata descendant must skip ambient site hooks")
                self.assertEqual(len(fixture.events(fixture.nonce, "leaf")), 1)
            finally:
                proc.terminate()
                proc.communicate(timeout=4)


class OrdinaryProbeReadiness(unittest.TestCase):
    class Fixture:
        def __init__(self, outcomes):
            self.outcomes = iter(outcomes)
            self.calls = 0
            self.nonce = "initial"
            self.rows = []

        def set_control(self, action, marker):
            self.marker = marker
            self.nonce = str(self.calls + 1)

        def cli(self, binary, arguments):
            self.calls += 1
            ready, self.rows = next(self.outcomes)
            return (self.marker.encode() if ready else b"version unavailable", b"")

        def events(self, nonce):
            return self.rows

    def test_cold_start_without_events_can_reach_an_exact_positive_control(self):
        import smoke_tui_metadata_pty as metadata
        row = {"action": "metadata", "argv": ["--version"]}
        fixture = self.Fixture([(False, []), (True, [row])])
        self.assertEqual(metadata.ordinary_control_probe(Path("owned"), fixture), [row])
        self.assertEqual(fixture.calls, 2)

    def test_a_probe_that_never_starts_fails_after_three_explicit_reads(self):
        import smoke_tui_metadata_pty as metadata
        fixture = self.Fixture([(False, [])] * 3)
        with self.assertRaises(AssertionError):
            metadata.ordinary_control_probe(Path("owned"), fixture)
        self.assertEqual(fixture.calls, 3)

    def test_an_observed_wrong_response_is_fatal_without_another_read(self):
        import smoke_tui_metadata_pty as metadata
        fixture = self.Fixture([(False, [{"action": "metadata"}]), (True, [])])
        with self.assertRaises(AssertionError):
            metadata.ordinary_control_probe(Path("owned"), fixture)
        self.assertEqual(fixture.calls, 1)
