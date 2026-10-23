"""CLI installation must work without desktop GUI build dependencies."""
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]


class InstallerBuildScope(unittest.TestCase):
    @unittest.skipUnless(os.name == "posix", "executed Unix installer fixture")
    def test_unix_installer_builds_only_cli_and_daemon_in_both_feature_modes(self):
        for features in (None, "personal-edition,accel"):
            with self.subTest(features=features), tempfile.TemporaryDirectory() as temp:
                root = Path(temp)
                (root / "tools").mkdir()
                (root / "bin").mkdir()
                shutil.copy2(ROOT / "install.sh", root / "install.sh")
                (root / "tools/toolchain-env.sh").write_text("")
                cargo = root / "bin/cargo"
                cargo.write_text('#!/bin/sh\nprintf "%s\\n" "$@" > "$FIXTURE_CARGO_ARGS"\nexit 91\n')
                cargo.chmod(0o755)
                arguments = root / "cargo-args"
                env = {"PATH": str(root / "bin") + os.pathsep + os.defpath,
                       "HOME": str(root / "home"), "FIXTURE_CARGO_ARGS": str(arguments)}
                expected = ["build", "--release", "--locked", "-p", "abbey", "--bins"]
                if features is not None:
                    env["ABBEY_CARGO_FEATURES"] = features
                    expected += ["--features", features]
                result = subprocess.run(["sh", "./install.sh"], cwd=root, env=env,
                                        capture_output=True, text=True)
                self.assertEqual(result.returncode, 91, result.stderr)
                self.assertEqual(arguments.read_text().splitlines(), expected)

    def test_powershell_build_commands_have_the_same_package_and_binary_scope(self):
        # Source contract only; this does not claim PowerShell/native acceptance.
        commands = [line.strip().split() for line in (ROOT / "install.ps1").read_text().splitlines()
                    if line.strip().startswith("cargo build ")]
        self.assertEqual(len(commands), 2)
        for command in commands:
            self.assertEqual(command[:7], ["cargo", "build", "--release", "--locked",
                                           "-p", "abbey", "--bins"])
