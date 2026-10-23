"""Behavioral fixtures for matched compiler tools and qualification admission."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import importlib.util
import unittest

ROOT = Path(__file__).resolve().parents[2]
POLICY_SPEC = importlib.util.spec_from_file_location('toolchain_policy', ROOT / 'tools/toolchain_policy.py')
POLICY = importlib.util.module_from_spec(POLICY_SPEC)
POLICY_SPEC.loader.exec_module(POLICY)
tomllib = POLICY.tomllib


class TomlReaderCompatibility(unittest.TestCase):
    def test_managed_python_structural_fallback(self):
        attempted = []
        sentinel = object()
        def importer(name):
            attempted.append(name)
            if name == 'pip._vendor.tomli':
                return sentinel
            raise ModuleNotFoundError(name)
        self.assertIs(POLICY.resolve_toml_reader(importer), sentinel)
        self.assertEqual(attempted, ['tomllib', 'tomli', 'pip._vendor.tomli'])

    def test_missing_structural_reader_fails_closed(self):
        def missing(name):
            raise ModuleNotFoundError(name)
        with self.assertRaisesRegex(RuntimeError, 'structural TOML reader'):
            POLICY.resolve_toml_reader(missing)



class ToolchainTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name).resolve()
        self.repo = self.root / 'repo'
        (self.repo / 'tools').mkdir(parents=True)
        shutil.copy(ROOT / 'tools/cargo.sh', self.repo / 'tools/cargo.sh')
        shutil.copy(ROOT / 'tools/toolchain_policy.py', self.repo / 'tools/toolchain_policy.py')
        shutil.copy(ROOT / 'tools/toolchain-env.sh', self.repo / 'tools/toolchain-env.sh')
        # The gate entrypoints source the gate lock from their own checkout,
        # so the fixture needs it beside the other tools they source.
        shutil.copy(ROOT / 'tools/gate_lock.sh', self.repo / 'tools/gate_lock.sh')
        shutil.copy(ROOT / 'rust-toolchain.toml', self.repo / 'rust-toolchain.toml')
        self.bin = self.root / 'selected/bin'
        self.bin.mkdir(parents=True)
        self.hostile = self.root / 'hostile'
        self.hostile.mkdir()
        self.script(self.hostile / 'cargo', "raise SystemExit('hostile cargo selected')")
        self.script(self.hostile / 'rustup', '''import os, pathlib
repo = pathlib.Path.cwd()
if not (repo / 'rust-toolchain.toml').exists():
    raise SystemExit('rustup resolved outside repository')
print(pathlib.Path(os.environ['SELECTED_BIN']) / sys.argv[-1])
''')
        self.script(self.bin / 'cargo', '''import os, shutil, subprocess
if sys.argv[1:] == ['--version']:
    print('cargo 1.99.1 fixture')
else:
    for tool in ['rustc','rustdoc','rustfmt','clippy-driver']:
        subprocess.run([tool, '--version'], check=True, stdout=subprocess.DEVNULL)
    print(json.dumps(dict(cwd=os.getcwd(), args=sys.argv[1:],
        tools={t: shutil.which(t) for t in ['rustc','rustdoc','rustfmt','clippy-driver']},
        cc=os.environ.get('CC'), cxx=os.environ.get('CXX'),
        dyld=os.environ['DYLD_FALLBACK_LIBRARY_PATH'], override=os.environ.get('RUSTUP_TOOLCHAIN'))))
''')
        for tool in ('rustc', 'rustdoc', 'rustfmt', 'clippy-driver'):
            self.script(self.bin / tool, "import os\nprint('rustc fixture\\nrelease: ' + os.environ.get('FAKE_RELEASE', '1.99.1') + '\\nhost: fixture-host')")
            self.script(self.hostile / tool, "raise SystemExit('hostile tool selected')")
        self.env = {k: v for k, v in os.environ.items() if k not in (
            'RUSTC', 'RUSTDOC', 'RUSTC_WRAPPER', 'RUSTC_WORKSPACE_WRAPPER',
            'RUSTC_BOOTSTRAP', 'RUSTUP_TOOLCHAIN', 'ABBEY_PYTHON', 'ABBEY_DYLD_FALLBACK_LIBRARY_PATH', 'CC', 'CXX', 'DYLD_FALLBACK_LIBRARY_PATH',
            'CARGO_BUILD_RUSTC', 'CARGO_BUILD_RUSTDOC', 'CARGO_BUILD_RUSTC_WRAPPER',
            'CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER')}
        self.env.update(PATH=f'{self.hostile}:{Path(sys.executable).parent}:/usr/bin:/bin',
                        SELECTED_BIN=str(self.bin), CARGO_HOME=str(self.root / 'cargo-home'))

    def script(self, path, body):
        path.write_text(f'#!{sys.executable}\nimport sys, json\n' + body + '\n')
        path.chmod(0o755)

    def run_wrapper(self, *args, **env):
        command = [str(self.repo / 'tools/cargo.sh'), *args]
        # macOS SIP strips DYLD variables at /bin/bash exec. Set the fixture
        # inside that shell, then source the executable wrapper without reexec.
        if 'DYLD_FALLBACK_LIBRARY_PATH' in env:
            command = ['/bin/bash', '-c',
                       'export DYLD_FALLBACK_LIBRARY_PATH=/existing; source "$@"',
                       'fixture', *command]
        return subprocess.run(command, cwd=self.root,
                              env=self.env | env, text=True, capture_output=True)

    def test_outside_repository_uses_matching_tools_and_preserves_cwd(self):
        result = self.run_wrapper('check', '--locked')
        self.assertEqual(result.returncode, 0, result.stderr)
        observed = json.loads(result.stdout)
        self.assertEqual(observed['cwd'], str(self.root))
        self.assertEqual(observed['args'], ['check', '--locked'])
        self.assertEqual(set(observed['tools'].values()), {str(self.bin / t) for t in observed['tools']})
        self.assertEqual(observed['cc'], '/usr/bin/cc')
        self.assertEqual(observed['cxx'], '/usr/bin/c++')
        self.assertEqual(observed['dyld'], f'{self.bin.parent}/lib:{Path.home()}/lib:/usr/local/lib:/usr/lib')

    def test_sourced_environment_preserves_live_loader_path_and_developer_overrides(self):
        result = self.run_wrapper('check', RUSTUP_TOOLCHAIN='developer', CC='/custom/cc',
                                  CXX='/custom/cxx', DYLD_FALLBACK_LIBRARY_PATH='/existing')
        self.assertEqual(result.returncode, 0, result.stderr)
        observed = json.loads(result.stdout)
        self.assertEqual(observed['override'], 'developer')
        self.assertEqual(observed['cc'], '/custom/cc')
        self.assertEqual(observed['cxx'], '/custom/cxx')
        self.assertEqual(observed['dyld'], f'{self.bin.parent}/lib:/existing')

    def test_qualification_records_full_compiler_and_rejects_other_release(self):
        good = self.run_wrapper('--abbey-qualify-toolchain')
        self.assertEqual(good.returncode, 0, good.stderr)
        self.assertIn('release: 1.99.1', good.stdout)
        self.assertIn(str(self.bin / 'rustc'), good.stdout)
        bad = self.run_wrapper('--abbey-qualify-toolchain', FAKE_RELEASE='1.100.0-nightly')
        self.assertNotEqual(bad.returncode, 0)

    def test_qualification_rejects_bootstrap_and_compiler_substitution(self):
        for variable in ('RUSTC_BOOTSTRAP', 'RUSTC', 'RUSTDOC', 'RUSTC_WRAPPER', 'RUSTC_WORKSPACE_WRAPPER', 'CARGO_BUILD_RUSTC', 'CARGO_BUILD_RUSTDOC',
                         'CARGO_BUILD_RUSTC_WRAPPER', 'CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER'):
            with self.subTest(variable=variable):
                result = self.run_wrapper('--abbey-qualify-toolchain', **{variable: 'substitute'})
                self.assertNotEqual(result.returncode, 0)
                self.assertIn(variable, result.stderr)

    def test_qualification_rejects_cargo_config_substitutions(self):
        for location in (self.root / '.cargo', self.root / 'cargo-home'):
            location.mkdir(exist_ok=True)
            config = location / 'config.toml'
            for content in ('[build]\nrustc = "/substitute"\n',
                            '[build]\nrustdoc = "/substitute"\n',
                            '[build]\nrustc-wrapper = "/substitute"\n',
                            '[build]\nrustc-workspace-wrapper = "/substitute"\n',
                            '[env]\nRUSTC_BOOTSTRAP = "1"\n',
                            '[env]\nPATH = { value = "/substitute", force = true }\n'):
                with self.subTest(location=location, content=content):
                    config.write_text(content)
                    result = self.run_wrapper('--abbey-qualify-toolchain')
                    self.assertNotEqual(result.returncode, 0)
                    self.assertIn('stable qualification rejects', result.stderr)
            config.write_text('[build]\njobs = 2\n')
            self.assertEqual(self.run_wrapper('--abbey-qualify-toolchain').returncode, 0)
            config.unlink()

    def test_repository_pin_and_components_are_exact(self):
        toolchain = tomllib.loads((ROOT / 'rust-toolchain.toml').read_text())['toolchain']
        self.assertEqual(toolchain['channel'], '1.99.1')
        self.assertEqual(toolchain['components'], ['rustfmt', 'clippy'])

    def test_resolution_failure_stops_before_cargo(self):
        self.script(self.hostile / 'rustup', "raise SystemExit('resolution refused')")
        result = self.run_wrapper('check')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('resolution refused', result.stderr)
        self.assertEqual(result.stdout, '')

    def test_invalid_compiler_configuration_fails_closed_with_diagnostic(self):
        location = self.root / '.cargo'
        location.mkdir()
        for content in ('[build]\nrustc = false\n',
                        '[build]\nrustdoc = ""\n',
                        'build = false\n', 'env = false\n',
                        '[env]\nRUSTC_BOOTSTRAP = false\n'):
            with self.subTest(content=content):
                (location / 'config.toml').write_text(content)
                result = self.run_wrapper('--abbey-qualify-toolchain')
                self.assertNotEqual(result.returncode, 0)
                self.assertIn('stable qualification', result.stderr)
                self.assertNotIn('Traceback', result.stderr)

    def test_bare_entrypoints_select_environment_before_building(self):
        for entrypoint in ('check.sh', 'desktop/check.sh', 'install.sh'):
            with self.subTest(entrypoint=entrypoint):
                destination = self.repo / entrypoint
                destination.parent.mkdir(exist_ok=True)
                (self.repo / 'desktop/scripts/tests').mkdir(parents=True, exist_ok=True)
                (self.repo / 'desktop/scripts/tests/test_fixture.py').write_text('import unittest\nclass Fixture(unittest.TestCase):\n def test_fixture(self): pass\n')
                shutil.copy(ROOT / entrypoint, destination)
                self.script(self.bin / 'cargo', "import os\nif sys.argv[1:] == ['--version']: print('cargo fixture'); raise SystemExit(0)\nprint('SELECTED:' + os.environ['DYLD_FALLBACK_LIBRARY_PATH'])\nraise SystemExit(73)")
                result = subprocess.run([str(destination)], cwd=self.root,
                                        env=self.env, capture_output=True, text=True)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn('SELECTED:' + str(self.bin.parent / 'lib'), result.stdout, result.stderr)
                self.assertNotIn('hostile cargo selected', result.stderr)

    def test_nested_shared_environment_preserves_policy_interpreter(self):
        result = subprocess.run(
            ['/bin/sh', '-c',
             'ABBEY_TOOLCHAIN_ROOT="$1"; . "$1/tools/toolchain-env.sh"; '
             '"$1/tools/cargo.sh" --abbey-qualify-toolchain',
             'fixture', str(self.repo)], cwd=self.root, env=self.env,
            capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('release: 1.99.1', result.stdout)

    def test_executable_preserves_explicit_loader_bridge(self):
        # Protected macOS shell interpreters strip incoming DYLD_* variables.
        # This ordinary variable survives the executable shell boundary.
        result = self.run_wrapper('check', ABBEY_DYLD_FALLBACK_LIBRARY_PATH='/existing')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(json.loads(result.stdout)['dyld'], f'{self.bin.parent}/lib:/existing')

    def test_nested_executable_preserves_sourced_loader_bridge(self):
        result = subprocess.run(
            ['/bin/bash', '-c',
             'export DYLD_FALLBACK_LIBRARY_PATH=/existing; '
             'ABBEY_TOOLCHAIN_ROOT="$1"; . "$1/tools/toolchain-env.sh"; '
             '"$1/tools/cargo.sh" check', 'fixture', str(self.repo)],
            cwd=self.root, env=self.env, capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(json.loads(result.stdout)['dyld'], f'{self.bin.parent}/lib:/existing')

    def test_installer_build_uses_toolchain_then_preserves_staged_fault_tools(self):
        shutil.copy(ROOT / 'install.sh', self.repo / 'install.sh')
        release = self.root / 'artifacts/release'
        release.mkdir(parents=True)
        self.script(release / 'abbey', """if sys.argv[1:] == ['edition', '--name']:
    print('abbey')
elif sys.argv[1:] == ['edition', '--daemon-name']:
    print('abbeyd')
else:
    print('fixture')
""")
        (release / 'libabi_metal_dot.dylib').write_bytes(b'fixture dylib')
        build_marker = self.root / 'matched-build'
        fault_marker = self.root / 'staged-fault'
        self.script(self.bin / 'cargo', f"""import os, pathlib, shutil
assert sys.argv[1:] == ['build', '--release', '--locked', '-p', 'abbey', '--bins', '--features', 'accel']
assert shutil.which('rustc') == {str(self.bin / 'rustc')!r}
assert os.environ['DYLD_FALLBACK_LIBRARY_PATH'].startswith({str(self.bin.parent / 'lib')!r})
pathlib.Path({str(build_marker)!r}).touch()
""")
        self.script(self.hostile / 'uname', "print('Darwin')")
        self.script(self.hostile / 'otool', f"""from pathlib import Path
if '.abbey-install.' in sys.argv[-1]:
    Path({str(fault_marker)!r}).touch()
    raise SystemExit(79)
print('@loader_path/libabi_metal_dot.dylib' + (' (fixture)' if sys.argv[1] == '-L' else ''))
""")
        destination = self.root / 'installation'
        destination.mkdir()
        old = {'abbey': b'prior binary', 'abbeyd': b'prior daemon',
               'libabi_metal_dot.dylib': b'prior dylib'}
        for name, content in old.items():
            (destination / name).write_bytes(content)
        result = subprocess.run([str(self.repo / 'install.sh')], cwd=self.root,
            env=self.env | {'CARGO_TARGET_DIR': str(release.parent),
                            'ABBEY_CARGO_FEATURES': 'accel',
                            'ABBEY_INSTALL_DIR': str(destination),
                            'ABBEY_COMPLETION_HOME': str(self.root / 'completion-home')},
            capture_output=True, text=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertTrue(build_marker.exists(), result.stderr)
        self.assertTrue(fault_marker.exists(), result.stderr)
        self.assertIn('staged ABI Metal dylib identity verification failed', result.stderr)
        self.assertEqual({p.name: p.read_bytes() for p in destination.iterdir()}, old)

    def test_initial_ci_path_system_python_can_qualify(self):
        if not Path('/usr/bin/python3').is_file():
            self.skipTest('managed system Python is unavailable')
        result = subprocess.run(
            ['/bin/sh', '-c',
             'PATH="/usr/bin:/bin:$PATH"; export PATH; '
             '"$1/tools/cargo.sh" --abbey-qualify-toolchain',
             'fixture', str(self.repo)], cwd=self.root, env=self.env,
            capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('release: 1.99.1', result.stdout)
