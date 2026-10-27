import importlib.util
import json
import os
import stat
import subprocess
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location(
    "source_manifest", ROOT / "tools" / "source_manifest.py"
)
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


def git(repo, *args):
    subprocess.run(
        ["git", "-C", str(repo), *args],
        check=True,
        capture_output=True,
        env={
            **os.environ,
            "GIT_AUTHOR_NAME": "t",
            "GIT_AUTHOR_EMAIL": "t@example.invalid",
            "GIT_COMMITTER_NAME": "t",
            "GIT_COMMITTER_EMAIL": "t@example.invalid",
        },
    )


def make_repo(directory):
    repo = Path(directory) / "repo"
    repo.mkdir()
    git(repo, "init", "-q")
    (repo / "run.sh").write_text("#!/bin/sh\n", encoding="utf-8")
    os.chmod(repo / "run.sh", 0o755)
    (repo / "plain.txt").write_text("plain\n", encoding="utf-8")
    git(repo, "add", "run.sh")
    git(repo, "commit", "-q", "-m", "init")
    # plain.txt stays untracked: untracked files must be recorded too.
    return repo


class SourceManifestTest(unittest.TestCase):
    def test_manifest_records_mode_and_hash_for_tracked_and_untracked_files(self):
        with tempfile.TemporaryDirectory() as directory:
            repo = make_repo(directory)
            manifest = MODULE.build_manifest([repo], environ={})
            files = manifest["repo"]["files"]
            self.assertEqual(0o755, stat.S_IMODE(files["run.sh"]["mode"]))
            self.assertIn("plain.txt", files)
            self.assertEqual(64, len(files["plain.txt"]["sha256"]))
            self.assertEqual(
                MODULE.sha256_file(repo / "plain.txt"), files["plain.txt"]["sha256"]
            )
            self.assertEqual(40, len(manifest["repo"]["head"]))
            for key in ("rustc", "cargo", "python", "env"):
                self.assertIn(key, manifest["environment"])
            json.dumps(manifest)

    def test_credential_like_environment_names_are_never_recorded(self):
        env = MODULE.capture_env(
            {
                "ABBEY_DAEMON_BEARER_TOKEN": "x",
                "ABBEY_API_KEY": "x",
                "ABBEY_CLIENT_SECRET": "x",
                "ABBEY_DB_PASSWORD": "x",
                "ABBEY_PROVIDER": "local",
                "CARGO_TARGET_DIR": "/t",
                "RUSTUP_TOOLCHAIN": "1.99.1",
                "HOME": "/home/x",
                "OPENAI_API_KEY": "x",
            }
        )
        self.assertEqual(
            {
                "ABBEY_PROVIDER": "local",
                "CARGO_TARGET_DIR": "/t",
                "RUSTUP_TOOLCHAIN": "1.99.1",
            },
            env,
        )

    def test_compare_reports_a_mode_only_change(self):
        with tempfile.TemporaryDirectory() as directory:
            repo = make_repo(directory)
            before = MODULE.build_manifest([repo], environ={})
            os.chmod(repo / "run.sh", 0o644)
            after = MODULE.build_manifest([repo], environ={})
            self.assertEqual(["repo/run.sh"], MODULE.compare_manifests(before, after))
            self.assertEqual([], MODULE.compare_manifests(before, before))
            a = Path(directory) / "a.json"
            b = Path(directory) / "b.json"
            a.write_text(json.dumps(before), encoding="utf-8")
            b.write_text(json.dumps(after), encoding="utf-8")
            self.assertEqual(1, MODULE.main(["--compare", str(a), str(b)]))
            self.assertEqual(0, MODULE.main(["--compare", str(a), str(a)]))


if __name__ == "__main__":
    unittest.main()
