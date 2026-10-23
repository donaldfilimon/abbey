import importlib.util
import json
import os
import sys
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location('service', Path(__file__).parents[1] / 'linux_user_service.py')
service = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(service)

class ServiceTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.home = Path(self.tmp.name).resolve()
        self.bin = self.home / 'bin space%$'
        self.bin.mkdir(mode=0o700)
        self.log = self.home / 'calls'
        self.systemctl = self.bin / 'systemctl'
        self.systemctl.write_text('#!' + sys.executable + ' -S\nimport os,sys,json\nwith open(os.environ["CALLS"],"a") as f: f.write(json.dumps(sys.argv[1:])+"\\n")\n')
        self.systemctl.chmod(0o700)
        self.make_cli('abbey', 'abbeyd', 'standard')
        self.env = patch.dict(os.environ, {'CALLS': str(self.log), 'ABBEYD_BEARER_TOKEN': 'must-not-leak'})
        self.env.start()
        self.addCleanup(self.env.stop)
    def make_cli(self, name, daemon, edition, protocol=1, schema=2):
        self.cli = self.bin / name
        self.cli.write_text('#!' + sys.executable + ' -S\nimport sys,json,os\nif sys.argv[1:]==["edition","--name"]: print('+repr(name)+')\nelif sys.argv[1:]==["edition","--daemon-name"]: print('+repr(daemon)+')\nelse:\n assert "ABBEYD_BEARER_TOKEN" not in os.environ\n print(json.dumps({"type":"status","payload":{"edition":'+repr(edition)+',"state":"ready","protocol_version":'+repr(protocol)+',"schema_version":'+repr(schema)+'}}))\n')
        self.cli.chmod(0o700)
        (self.bin / daemon).write_text('#!/bin/sh\nexit 0\n')
        (self.bin / daemon).chmod(0o700)
    def controller(self):
        c = service.Controller(self.cli, self.home, self.home, self.systemctl, platform='linux')
        c.env['CALLS'] = str(self.log)
        return c
    def test_status_accepts_exact_supported_pairs_both_editions(self):
        for name, daemon, edition in [('abbey','abbeyd','standard'), ('abbey-personal','abbey-personal-daemon','personal')]:
            self.make_cli(name, daemon, edition)
            c = self.controller(); c.install()
            for protocol, schema in [(1, 2), (2, 3)]:
                with self.subTest(edition=edition, protocol=protocol, schema=schema):
                    self.make_cli(name, daemon, edition, protocol, schema)
                    event = c.status()
                    self.assertEqual((event['payload']['protocol_version'], event['payload']['schema_version']), (protocol, schema))

    def test_status_rejects_crossed_unknown_and_noninteger_versions(self):
        c = self.controller(); c.install()
        for protocol, schema in [(1, 3), (2, 2), (1, 1), (2, 4), (3, 3), (0, 2), ('2', 3), (2, '3'), (True, 2), (2, True)]:
            with self.subTest(protocol=protocol, schema=schema):
                self.make_cli('abbey','abbeyd','standard', protocol, schema)
                with self.assertRaises(service.ServiceError): c.status()
        self.make_cli('abbey','abbeyd','personal', 2, 3)
        with self.assertRaises(service.ServiceError): c.status()

    def eager_identity(self, name, daemon, edition, fail=False):
        self.make_cli(name, daemon, edition)
        original = self.cli.read_text()
        log = self.home / ('identity-' + name + '.jsonl')
        initialization = ('import os,json,sys\nfrom pathlib import Path\n'
            + 'if sys.argv[1] == "edition":\n'
            + ' keys = ["HOME","USERPROFILE","XDG_CONFIG_HOME","XDG_STATE_HOME","ABBEY_STATE_DIR","ABBEY_PERSONAL_STATE_DIR","ABBEY_CONFIG","ABBEY_PERSONAL_CONFIG"]\n'
            + ' with open(' + repr(str(log)) + ', "a") as f: f.write(json.dumps({k:os.environ.get(k) for k in keys}) + "\\n")\n'
            + ' state = Path(os.environ.get(' + repr('ABBEY_PERSONAL_STATE_DIR' if edition == 'personal' else 'ABBEY_STATE_DIR') + ', str(Path.home()/".local/state"/' + repr(name) + ')))\n'
            + ' state.mkdir(parents=True,exist_ok=True,mode=0o755)\n (state/"by-cwd").mkdir(exist_ok=True,mode=0o755)\n'
            + (' sys.exit(1)\n' if fail else ''))
        self.cli.write_text(original.split('\n', 1)[0] + '\n' + initialization + original.split('\n', 1)[1])
        return log

    def test_eager_identity_isolated_for_both_editions(self):
        for name, daemon, edition in [('abbey','abbeyd','standard'), ('abbey-personal','abbey-personal-daemon','personal')]:
            with self.subTest(edition=edition):
                log = self.eager_identity(name, daemon, edition)
                c = self.controller()
                self.assertFalse(c.state.exists(), 'identity polluted intended service state')
                probes = [json.loads(row) for row in log.read_text().splitlines()]
                self.assertEqual(len(probes), 2)
                for env in probes:
                    self.assertNotEqual(env['HOME'], str(self.home))
                    self.assertTrue(all(env.values()))
                    self.assertFalse(Path(env['HOME']).exists())
                    self.assertFalse(Path(env['ABBEY_STATE_DIR']).exists())
                    self.assertFalse(Path(env['ABBEY_PERSONAL_STATE_DIR']).exists())
                self.assertEqual(c.service_env['HOME'], str(self.home))
                self.assertNotIn('ABBEY_CONFIG', c.service_env)
                self.assertNotIn('ABBEY_PERSONAL_CONFIG', c.service_env)
                self.assertNotIn('ABBEY_STATE_DIR' if edition == 'personal' else 'ABBEY_PERSONAL_STATE_DIR', c.service_env)
                self.assertNotIn('USERPROFILE', c.service_env)
                self.assertEqual(c.service_env['ABBEY_PERSONAL_STATE_DIR' if edition == 'personal' else 'ABBEY_STATE_DIR'], str(c.state))
                c.install(); self.assertEqual(c.state.stat().st_mode & 0o777, 0o700)

    def test_eager_identity_failure_cleans_probe_roots(self):
        log = self.eager_identity('abbey','abbeyd','standard', fail=True)
        with self.assertRaises(service.ServiceError): self.controller()
        env = json.loads(log.read_text().splitlines()[0])
        self.assertNotEqual(env['HOME'], str(self.home))
        self.assertFalse(Path(env['HOME']).exists())
        self.assertFalse((self.home/'.local/state/abbey').exists())

    def test_eager_unknown_identity_cleans_probe_roots(self):
        log = self.eager_identity('unknown','unknown-daemon','standard')
        with self.assertRaisesRegex(service.ServiceError, 'unknown or inconsistent'):
            self.controller()
        probes = [json.loads(row) for row in log.read_text().splitlines()]
        self.assertEqual(len(probes), 2)
        for env in probes:
            self.assertFalse(Path(env['HOME']).exists())
            self.assertFalse(Path(env['ABBEY_STATE_DIR']).exists())
        self.assertFalse((self.home/'.local/state/unknown').exists())

    def test_eager_identity_preserves_existing_permissive_state(self):
        state = self.home/'.local/state/abbey'
        state.mkdir(parents=True,mode=0o755);state.chmod(0o755)
        (state/'sentinel').write_text('retain')
        self.eager_identity('abbey','abbeyd','standard')
        c = self.controller()
        with self.assertRaises(service.ServiceError): c.install()
        self.assertEqual(state.stat().st_mode & 0o777, 0o755)
        self.assertEqual((state/'sentinel').read_text(), 'retain')
        self.assertFalse((state/'by-cwd').exists())

    def test_install_retains_secret_and_uninstall_state(self):
        c = self.controller(); c.install()
        secret = c.token.read_bytes(); unit = c.unit.read_text()
        self.assertNotIn(secret.decode(), unit)
        self.assertIn('%%', unit); self.assertIn('UMask=0077', unit)
        self.assertFalse(self.log.exists() and 'start' in self.log.read_text())
        c.install(); self.assertEqual(secret, c.token.read_bytes())
        (c.state / 'runtime.sqlite').write_text('retained')
        c.start(); c.uninstall()
        self.assertFalse(c.unit.exists()); self.assertTrue(c.token.exists())
        self.assertTrue((c.state / 'runtime.sqlite').exists())
    def test_personal_and_wrong_readiness(self):
        self.make_cli('abbey-personal', 'abbey-personal-daemon', 'standard')
        c = self.controller(); c.install()
        with self.assertRaises(service.ServiceError): c.start()
        self.assertIn('personal', str(c.unit)); self.assertTrue(c.unit.exists())
    def test_platform_refusal(self):
        with self.assertRaises(service.ServiceError):
            service.Controller(self.cli, self.home, self.home, self.systemctl, platform='darwin')
    def test_unknown_unit_and_insecure_token(self):
        c = self.controller(); c.install()
        c.token.chmod(0o644)
        with self.assertRaises(service.ServiceError): c.install()
        c.token.chmod(0o600); c.unit.write_text('unrelated')
        with self.assertRaises(service.ServiceError): c.uninstall()
    def test_symlink_and_publication_rollback(self):
        c = self.controller(); c.install(); old = c.unit.read_bytes()
        with patch.object(service.os, 'replace', side_effect=OSError('fixture failure')):
            with self.assertRaises(OSError): c.install()
        self.assertEqual(old, c.unit.read_bytes())
        c.token.unlink(); c.token.symlink_to(self.cli)
        with self.assertRaises(service.ServiceError): c.status()

    def fail_reloads(self, c):
        self.systemctl.write_text('#!' + sys.executable + ' -S\nimport sys\nif "daemon-reload" in sys.argv: sys.exit(1)\n')

    def test_reload_failure_removes_new_unit_and_retains_credentials(self):
        c = self.controller()
        self.fail_reloads(c)
        with self.assertRaises(service.ServiceError): c.install()
        self.assertFalse(c.unit.exists())
        self.assertTrue(c.token.exists()); self.assertTrue(c.state.is_dir())
        self.assertEqual(c.token.stat().st_mode & 0o777, 0o600)

    def test_reload_failure_restores_previous_unit_bytes_and_mode(self):
        c = self.controller(); c.install()
        c.unit.chmod(0o400)
        previous = c.unit.read_bytes(); token = c.token.read_bytes()
        (c.state / 'runtime.sqlite').write_text('retained')
        self.fail_reloads(c)
        with self.assertRaises(service.ServiceError): c.install()
        self.assertEqual(c.unit.read_bytes(), previous)
        self.assertEqual(c.unit.stat().st_mode & 0o777, 0o400)
        self.assertEqual(c.token.read_bytes(), token)
        self.assertEqual((c.state / 'runtime.sqlite').read_text(), 'retained')

    def test_reload_failure_reports_failed_rollback(self):
        c = self.controller()
        self.fail_reloads(c)
        publish = c.publish_unit
        with patch.object(c, 'publish_unit', side_effect=lambda *args: publish(*args)):
            with patch.object(Path, 'unlink', side_effect=OSError('fixture rollback failure')):
                with self.assertRaisesRegex(service.ServiceError, 'unit rollback failed'):
                    c.install()
        self.assertTrue(c.unit.exists()); self.assertTrue(c.token.exists())

    def test_reload_failure_reports_restored_but_reload_failed(self):
        c = self.controller(); self.fail_reloads(c)
        with self.assertRaisesRegex(service.ServiceError, 'state restored; rollback daemon-reload failed'):
            c.install()
        self.assertFalse(c.unit.exists()); self.assertTrue(c.token.exists())

    def test_reload_failure_reports_successful_rollback_reload(self):
        c = self.controller()
        marker = self.home / 'reload-once'
        self.systemctl.write_text('#!' + sys.executable + ' -S\nimport sys\nfrom pathlib import Path\np = Path(' + repr(str(marker)) + ')\nif "daemon-reload" in sys.argv and not p.exists():\n p.touch()\n sys.exit(1)\n')
        with self.assertRaisesRegex(service.ServiceError, 'state restored and reloaded'):
            c.install()
        self.assertFalse(c.unit.exists()); self.assertTrue(c.token.exists())

    def test_missing_binary_and_unknown_identity(self):
        (self.bin / 'abbeyd').unlink()
        with self.assertRaises((service.ServiceError, OSError)): self.controller()
        self.make_cli('unknown', 'unknown-daemon', 'standard')
        with self.assertRaises(service.ServiceError): self.controller()

    def test_personal_ready_and_isolated(self):
        safe = self.controller(); safe.install()
        self.make_cli('abbey-personal', 'abbey-personal-daemon', 'personal')
        personal = self.controller(); personal.install(); personal.start()
        self.assertNotEqual(safe.token, personal.token)
        personal.uninstall(); self.assertTrue(safe.unit.exists())
        calls = self.log.read_text()
        self.assertNotIn(safe.token.read_text(), calls)
        self.assertNotIn(personal.token.read_text(), calls)
        self.assertNotIn('enable', calls)

    def test_bounded_output_and_foreign_owner(self):
        noisy = self.bin / 'noisy'
        noisy.write_text('#!' + sys.executable + ' -S\nprint("x" * 70000)\n')
        noisy.chmod(0o700)
        with self.assertRaises(service.ServiceError): service.run([noisy], {'PATH': '/usr/bin:/bin'})
        with patch.object(service.os, 'getuid', return_value=987654):
            with self.assertRaises(service.ServiceError): service.checked(self.cli)

    def test_root_sticky_temp_ancestor(self):
        with tempfile.TemporaryDirectory(dir='/tmp') as scratch:
            directory = Path(scratch).resolve()
            self.assertEqual(service.checked(directory, directory=True), directory)

    def test_directory_permissions_and_quotes(self):
        c = self.controller(); c.install()
        c.config.chmod(0o755)
        with self.assertRaises(service.ServiceError): c.start()
        self.assertEqual(service.quote('a$%"'), '\"a$%%\\\"\"')
        self.assertEqual(service.quote('$', executable=True), '\"$$\"')

if __name__ == '__main__': unittest.main()
