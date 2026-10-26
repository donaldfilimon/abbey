"""Bound the attributed import and isolated Cargo package without running Bot."""
import importlib
import json
from pathlib import Path
import subprocess
import unittest


def resolve_toml_reader(importer=importlib.import_module):
    """Use an available structural TOML parser, including Xcode Python's pip."""
    for module_name in ('tomllib', 'tomli', 'pip._vendor.tomli'):
        try:
            return importer(module_name)
        except ModuleNotFoundError:
            continue
    raise RuntimeError(
        'the Bot boundary check requires Python 3.11+ or a managed Python '
        'runtime providing tomli; no structural TOML reader is available'
    )


tomllib = resolve_toml_reader()

ROOT = Path(__file__).resolve().parents[2]
BOT = ROOT / 'extensions/bot'


def git(*args):
    return subprocess.check_output(['git', '-C', str(ROOT), *args], text=True).strip()


class BotSourceBoundary(unittest.TestCase):
    def test_attributed_tree_and_original_history_survive_import(self):
        receipt = json.loads((ROOT / 'docs/verification/2026-10-25-bot-source-import.json').read_text())
        source, imported = receipt['source_commit'], receipt['import_commit']
        self.assertEqual(git('rev-parse', source + '^{tree}'), receipt['source_tree'])
        self.assertEqual(git('rev-parse', imported + ':extensions/bot'), receipt['source_tree'])
        subprocess.run(['git', '-C', str(ROOT), 'merge-base', '--is-ancestor', source, 'HEAD'], check=True)
        self.assertEqual(git('rev-parse', imported + '^2'), source)
        self.assertNotIn('160000 ', git('ls-tree', '-r', imported, 'extensions/bot'))
        self.assertFalse(any(path.name == '.git' for path in BOT.rglob('.git')))

    def test_all_64_source_requirements_remain_verbatim(self):
        import re
        families = ['ACTIVITY-LAUNCH', 'VOICE-COMPLETION', 'TEXT-RELIABILITY',
                    'LEARNING-QUALITY', 'CONTINUITY', 'INITIATIVE',
                    'MEMBER-WORKFLOWS', 'RELEASE-QUALIFICATION']
        expected = {f'{family}-R{number}' for family in families for number in range(1, 9)}
        found = set()
        for path in (BOT / 'docs/superpowers/specs').glob('2026-10-01-mlai-*-design.md'):
            original = git('show', '6eb5cff114c22dd7a2d0f78f98c2a1c2df8bf215:' + path.relative_to(BOT).as_posix())
            self.assertEqual(path.read_text().strip(), original)
            found.update(re.findall(r'^([A-Z-]+-R[1-8]):', original, re.MULTILINE))
        self.assertEqual(found & expected, expected)

    def test_abbey_defaults_do_not_admit_bot_graph(self):
        manifest = tomllib.loads((ROOT / 'Cargo.toml').read_text())
        self.assertIn('extensions/bot', manifest['workspace']['exclude'])
        self.assertNotIn('extensions/bot', manifest['workspace']['members'])
        self.assertNotIn('bot-extension', manifest['features'])
        self.assertFalse(any(dep.get('path', '').startswith('extensions/bot')
                             for dep in manifest['dependencies'].values() if isinstance(dep, dict)))
        # The lock may grow only by first-party workspace packages (no
        # registry/git source); nothing from Bot's separate graph may enter.
        def packages(text):
            return {(p['name'], p['version']): p.get('source')
                    for p in tomllib.loads(text)['package']}
        current = packages((ROOT / 'Cargo.lock').read_text())
        baseline = packages(git('show', '80187be345828ed966374c5cd6880eb40fe62bec:Cargo.lock'))
        bot_only = set(packages((BOT / 'Cargo.lock').read_text())) - set(baseline)
        added = set(current) - set(baseline)
        self.assertNotIn('abbey-bot', {name for name, _ in current})
        self.assertFalse(added & bot_only)
        self.assertEqual({key for key in added if current[key] is not None}, set())

    def test_real_library_and_single_compatibility_entry(self):
        manifest = tomllib.loads((BOT / 'Cargo.toml').read_text())
        self.assertEqual(manifest['lib']['path'], 'src/lib.rs')
        library = (BOT / 'src/lib.rs').read_text()
        self.assertIn('pub struct Data', library)
        for module in ['runtime', 'gateway', 'pipeline', 'service', 'persist']:
            self.assertIn('mod ' + module + ';', library)
        main = (BOT / 'src/main.rs').read_text()
        self.assertIn('abbey_bot::standalone::run()', main)
        self.assertNotIn('mod runtime', main)
        self.assertIn('pub unsafe fn run()', (BOT / 'src/standalone.rs').read_text())
        self.assertIn('serenity', manifest['patch']['crates-io'])
        self.assertIn('openmls_rust_crypto', manifest['patch']['crates-io'])
        self.assertEqual((BOT / 'Cargo.lock').read_text(), git('show', '6eb5cff114c22dd7a2d0f78f98c2a1c2df8bf215:Cargo.lock') + '\n')


if __name__ == '__main__':
    unittest.main()
