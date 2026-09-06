import copy
import hashlib
import json
from pathlib import Path
import sys
import unittest
import tempfile
import socket
from unittest.mock import Mock, patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from routes_assertions import WORKSPACES, ProofFailure, empty, inspect, populated, rejected
from prove_routes_macos import Smoke, accepting_socket


def snapshot(*texts):
    return {"complete": True, "nodes": [{"role": "AXStaticText", "attributes": {"AXValue": [text]}} for text in texts]}


def valid_page():
    digests = ["ws-" + hashlib.sha256(b"abbey:route-audit-workspace:v1\0" + p.encode()).hexdigest()[:12] for p in WORKSPACES]
    return snapshot("25 decision(s) across 3 workspace(s)", "route-stage route-correlation route-tool", *digests, *(f"82% ROUTE_ROW_{i:03} [path] [path] [path]" for i in reversed(range(30, 55))))


class Assertions(unittest.TestCase):
    def test_positive_populated_empty_and_error_states(self):
        populated(valid_page(), 25, ["scratch-secret"])
        empty(snapshot("No routing has been audited"), ["scratch-secret"])
        rejected(snapshot("Cannot reach Abbey"), ["scratch-secret"])

    def test_missing_row_fails(self):
        page = valid_page()
        page["nodes"].pop()
        with self.assertRaisesRegex(ProofFailure, "route_rows_or_order"):
            populated(page, 25, [])

    def test_reordered_or_duplicate_rows_fail(self):
        for duplicate in (False, True):
            page = valid_page()
            page["nodes"][-2:] = list(reversed(page["nodes"][-2:]))
            if duplicate:
                page["nodes"].append(copy.deepcopy(page["nodes"][-1]))
            with self.assertRaises(ProofFailure):
                populated(page, 25, [])

    def test_sensitive_text_in_any_attribute_fails_without_echo(self):
        for attribute in ("AXValue", "AXTitle", "AXDescription", "AXHelp", "AXUnknownFutureAttribute"):
            for secret in (WORKSPACES[0], "SECRET_UNIX_PATH", "SECRET_WINDOWS_PATH", "SECRET_HOME_PATH", "scratch-secret", "LOCAL_FALLBACK_CANARY"):
                page = valid_page()
                page["nodes"][0]["attributes"][attribute] = [secret]
                with self.assertRaisesRegex(ProofFailure, "^rendered_sensitive_data$"):
                    inspect(page, ["scratch-secret"])

    def test_error_and_empty_cannot_keep_stale_rows(self):
        for assertion, label in ((rejected, "Cannot reach Abbey"), (empty, "No routing has been audited")):
            with self.assertRaises(ProofFailure):
                assertion(snapshot(label, "ROUTE_ROW_054"), [])

    def test_truncated_or_empty_tree_fails(self):
        for page in ({"complete": False, "nodes": valid_page()["nodes"]}, {"complete": True, "nodes": []}):
            with self.assertRaises(ProofFailure):
                inspect(page, [])

    def test_missing_digest_or_projection_fails(self):
        for index in (1, 2):
            page = valid_page()
            page["nodes"].pop(index)
            with self.assertRaises(ProofFailure):
                populated(page, 25, [])

    def test_authentication_requires_the_correct_error_kind(self):
        with self.assertRaises(ProofFailure):
            rejected(snapshot("Daemon configuration error"), [], "Request rejected")

    def test_container_text_is_not_a_rendered_row(self):
        page = valid_page()
        page["nodes"][-1]["role"] = "AXGroup"
        with self.assertRaises(ProofFailure):
            populated(page, 25, [])

    def test_loading_or_blank_cannot_pass_as_error(self):
        with self.assertRaises(ProofFailure):
            rejected(snapshot("loading…"), [])


class ScratchIsolation(unittest.TestCase):
    def test_daemon_readiness_requires_listen_not_just_bind(self):
        with tempfile.TemporaryDirectory() as root:
            path = Path(root) / "daemon.sock"
            with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as listener:
                listener.bind(str(path))
                path.chmod(0o600)
                self.assertFalse(accepting_socket(path))
                listener.listen(4)
                self.assertTrue(accepting_socket(path))
            self.assertFalse(accepting_socket(path))

    def test_discarded_traversal_strings_are_scanned(self):
        with tempfile.TemporaryDirectory() as root:
            proof = Smoke(Path(root), Path(root) / "driver", {})
            envelope = {"snapshots": [valid_page()],
                        "observedStrings": [proof.bearer], "discardedSnapshots": 1}
            result = Mock(returncode=0, stdout=json.dumps(envelope).encode(), stderr=b"")
            process = Mock(pid=123)
            process.poll.return_value = None
            with patch("prove_routes_macos.subprocess.run", return_value=result):
                with self.assertRaisesRegex(ProofFailure, "^rendered_sensitive_data$"):
                    proof.ax(process)

    def test_scratch_environment_and_route_hash_guard(self):
        with tempfile.TemporaryDirectory() as root:
            scratch = Path(root)
            proof = Smoke(scratch, scratch / "driver", {"completed_scenarios": []})
            with patch.dict("os.environ", {"ABBEY_MODEL_RUNTIME_CONFIG": "/live/config", "ABBEYD_BEARER_TOKEN_FILE": "/live/token"}):
                env = proof.env("desktop", scratch / "daemon.sock", proof.bearer)
            self.assertNotIn("ABBEY_MODEL_RUNTIME_CONFIG", env)
            self.assertNotIn("ABBEYD_BEARER_TOKEN_FILE", env)
            self.assertEqual(env["ABBEY_STATE_DIR"], str(scratch / "desktop"))
            self.assertNotEqual(proof.bearer, proof.wrong)
            proof.verify_files()
            (scratch / "daemon" / "route.jsonl").write_text("")
            with self.assertRaisesRegex(ProofFailure, "^route_file_mutated$"):
                proof.verify_files()


if __name__ == "__main__":
    unittest.main()
