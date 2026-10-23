"""Executed installer fixture: accelerator editions cannot replace each other.

Tool stubs model Mach-O metadata; the native smoke separately verifies real
linked binaries. No build, daemon, GPU, or user profile is used here.
"""
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]


@unittest.skipUnless(os.name == "posix", "Unix installer fixture")
class AcceleratorEditionInstall(unittest.TestCase):
    def test_both_install_orders_keep_other_editions_library_bytes(self):
        for order in (("abbey", "abbey-personal"), ("abbey-personal", "abbey")):
            with self.subTest(order=order), tempfile.TemporaryDirectory() as temp:
                root = Path(temp)
                (root / "tools").mkdir()
                shutil.copy2(ROOT / "install.sh", root / "install.sh")
                # Preserve the script's build environment contract, without Cargo.
                (root / "tools/toolchain-env.sh").write_text("")
                release = root / "target/release"
                release.mkdir(parents=True)
                tools = root / "bin"
                tools.mkdir()
                scripts = {
                    "cargo": "exit 0",
                    "uname": "echo Darwin",
                    "otool": 'case "$1" in -D) head -n 1 "$2";; -L) cat "$2";; esac',
                    "install_name_tool": 'if [ "$1" = -id ]; then tail -n +2 "$3" > "$3.body"; { printf "%s\\n" "$2"; cat "$3.body"; } > "$3"; rm "$3.body"; fi',
                    "codesign": 'test "${FIXTURE_FAIL_SIGN:-0}" = 0',
                }
                for name, body in scripts.items():
                    path = tools / name
                    path.write_text("#!/bin/sh\n" + body + "\n")
                    path.chmod(0o755)
                env = {"PATH": str(tools) + os.pathsep + os.defpath,
                            "HOME": str(root / "home"),
                            "ABBEY_INSTALL_DIR": str(root / "installed"),
                            "ABBEY_COMPLETION_HOME": str(root / "home"),
                            "ABBEY_CARGO_FEATURES": "accel",
                            "CARGO_TARGET_DIR": str(root / "target"),
                            "ABBEY_CONFIG": str(root / "safe.toml"),
                            "ABBEY_PERSONAL_CONFIG": str(root / "personal.toml"),
                            "ABBEY_STATE_DIR": str(root / "safe-state"),
                            "ABBEY_PERSONAL_STATE_DIR": str(root / "personal-state")}
                installed = root / "installed"
                snapshots = {}
                for edition in order:
                    # Shell executables stand in for the release CLI; its otool
                    # projection includes both possible load commands.
                    cli = release / "abbey"
                    cli.write_text(f'''#!/bin/sh
# @loader_path/libabi_metal_dot.dylib (compatibility version 0.0.0)
# @loader_path/libabbey-personal-abi-metal.dylib (compatibility version 0.0.0)
case "$1 $2" in
 "edition --name") echo {edition};;
 "edition --daemon-name") echo {"abbeyd" if edition == "abbey" else "abbey-personal-daemon"};;
 "accel verify") echo '{{"kernels_linked": true}}';;
 *) echo fixture;;
esac
''')
                    cli.chmod(0o755)
                    (release / "libabi_metal_dot.dylib").write_text(
                        "@loader_path/libabi_metal_dot.dylib\n" + edition + "\n")
                    if edition == "abbey-personal":
                        failed = subprocess.run(
                            ["./install.sh"], cwd=root,
                            env={**env, "FIXTURE_FAIL_SIGN": "1"},
                            capture_output=True, text=True,
                        )
                        self.assertNotEqual(failed.returncode, 0)
                        for path, contents in snapshots.items():
                            self.assertEqual(path.read_bytes(), contents)
                        self.assertFalse(list(installed.glob(".abbey-*")))
                        self.assertFalse(list(installed.glob("*personal*")))
                    subprocess.run(["./install.sh"], cwd=root, env=env,
                                   capture_output=True, text=True, check=True)
                    for path, contents in snapshots.items():
                        self.assertEqual(path.read_bytes(), contents,
                                         "install changed the other edition's library")
                    dylibs = list(installed.glob("*.dylib"))
                    self.assertEqual(len(dylibs), len(snapshots) + 1,
                                     "each edition must own a distinct dylib")
                    for path in dylibs:
                        snapshots[path] = path.read_bytes()
