"""Independent mutation controls for Abbey's three reviewed vendor copies."""
import importlib.util
from pathlib import Path
import json
import shutil
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[3]
spec = importlib.util.spec_from_file_location('gtk_vendor', ROOT / 'tools/security/check_gtk_vendor.py')


def setUpModule():
    """Initialize scratch before either class runs, including before Cargo builds."""
    (ROOT / '.work/gtk').mkdir(parents=True, exist_ok=True)

class VendorTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.check = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(cls.check)

    def test_reviewed_sources(self):
        self.check.verify_sources(ROOT / 'vendor')

    def mutate(self, file, change, rebless=False):
        with tempfile.TemporaryDirectory(dir=ROOT / '.work/gtk') as tmp:
            vendor = Path(tmp) / 'vendor'
            shutil.copytree(ROOT / 'vendor', vendor, ignore=shutil.ignore_patterns('target'))
            path = vendor / file
            path.write_bytes(change(path.read_bytes()))
            if rebless:
                manifest = vendor / 'source-manifest.json'
                data = json.loads(manifest.read_text())
                crate, rel = file.split('/', 1)
                data['crates'][crate]['patched_files'][rel] = self.check.sha(path.read_bytes())
                manifest.write_text(json.dumps(data))
            with self.assertRaises(ValueError):
                self.check.verify_sources(vendor)

    def test_dev_harness_provenance_substitution_rejected(self):
        with tempfile.TemporaryDirectory(dir=ROOT / '.work/gtk') as tmp:
            vendor = Path(tmp) / 'vendor'
            shutil.copytree(ROOT / 'vendor', vendor)
            manifest = vendor / 'source-manifest.json'
            data = json.loads(manifest.read_text())
            data['dev_harness'] = {'package': 'substitute'}
            manifest.write_text(json.dumps(data))
            with self.assertRaises(ValueError):
                self.check.verify_sources(vendor)

    def test_license_drift(self):
        self.mutate('glib-0.18.5/LICENSE', lambda b: b + b'changed')

    def test_vcs_drift(self):
        self.mutate('gtk3-macros-0.18.2/.cargo_vcs_info.json', lambda b: b.replace(b'true', b'false'))

    def test_version_substitution(self):
        self.mutate('glib-0.18.5/Cargo.toml', lambda b: b.replace(b'0.18.5', b'0.18.6'))

    def test_self_blessed_source_rejected(self):
        self.mutate('glib-0.18.5/src/variant_iter.rs', lambda b: b + b'\n// drift\n', True)

    def test_old_macro_dependency_rejected(self):
        self.mutate('glib-macros-0.18.5/Cargo.toml', lambda b: b.replace(b'proc-macro-error3', b'proc-macro-error'))

    def test_crate_directory_symlink_rejected(self):
        with tempfile.TemporaryDirectory(dir=ROOT / '.work/gtk') as tmp:
            vendor = Path(tmp) / 'vendor'
            vendor.mkdir()
            shutil.copyfile(ROOT / 'vendor/source-manifest.json', vendor / 'source-manifest.json')
            for crate in self.check.PINS:
                (vendor / crate).symlink_to(ROOT / 'vendor' / crate, target_is_directory=True)
            with self.assertRaises(ValueError):
                self.check.verify_sources(vendor)

    def test_vendor_directory_symlink_rejected(self):
        with tempfile.TemporaryDirectory(dir=ROOT / '.work/gtk') as tmp:
            vendor = Path(tmp) / 'vendor'
            vendor.symlink_to(ROOT / 'vendor', target_is_directory=True)
            with self.assertRaises(ValueError):
                self.check.verify_sources(vendor)

    def test_extra_file_rejected(self):
        with tempfile.TemporaryDirectory(dir=ROOT / '.work/gtk') as tmp:
            vendor = Path(tmp) / 'vendor'
            shutil.copytree(ROOT / 'vendor', vendor)
            (vendor / 'glib-0.18.5/extra.rs').write_text('// unexpected')
            with self.assertRaises(ValueError):
                self.check.verify_sources(vendor)


class ResolutionTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.check = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(cls.check)

    def metadata(self):
        packages = []
        for crate in self.check.PINS:
            name, version = crate.rsplit('-', 1)
            packages.append(dict(name=name, version=version, id=name, source=None, manifest_path=str(ROOT / 'vendor' / crate / 'Cargo.toml')))
        for name, (version, _) in self.check.REPLACEMENTS.items():
            packages.append(dict(name=name, version=version, id=name, source='registry+https://github.com/rust-lang/crates.io-index'))
        nodes = [dict(id=name, deps=[dict(name='proc_macro_error3', pkg='proc-macro-error3')]) for name in ['glib-macros', 'gtk3-macros']]
        packages.append(dict(name='syn', version='2.0.119', id='syn2'))
        nodes.append(dict(id='proc-macro-error3', features=['syn2-error'], deps=[dict(name='syn2', pkg='syn2')]))
        return dict(packages=packages, resolve=dict(nodes=nodes))

    def test_expected_resolution(self):
        self.check.verify_metadata(self.metadata(), ROOT / 'vendor')

    def test_registry_substitution_rejected(self):
        data = self.metadata()
        data['packages'][0]['source'] = 'registry+https://github.com/rust-lang/crates.io-index'
        with self.assertRaises(ValueError):
            self.check.verify_metadata(data, ROOT / 'vendor')

    def test_sibling_vendor_substitution_rejected(self):
        data = self.metadata()
        data['packages'][0]['manifest_path'] = str(ROOT.parent / 'wdbx/vendor/glib-0.18.5/Cargo.toml')
        with self.assertRaises(ValueError):
            self.check.verify_metadata(data, ROOT / 'vendor')

    def test_dependency_alias_rejected(self):
        data = self.metadata()
        data['resolve']['nodes'][1]['deps'][0]['name'] = 'proc_macro_error'
        with self.assertRaises(ValueError):
            self.check.verify_metadata(data, ROOT / 'vendor')

    def test_missing_macro_parent_node_rejected(self):
        data = self.metadata()
        data['resolve']['nodes'].pop(0)
        with self.assertRaises(ValueError):
            self.check.verify_metadata(data, ROOT / 'vendor')

    def test_syn3_default_rejected(self):
        data = self.metadata()
        data['resolve']['nodes'][-1]['features'] += ['default', 'syn3-error']
        with self.assertRaises(ValueError):
            self.check.verify_metadata(data, ROOT / 'vendor')

    def test_superseded_proc2_rejected(self):
        data = self.metadata()
        data['packages'].append(dict(name='proc-macro-error2', version='2.0.1'))
        with self.assertRaises(ValueError):
            self.check.verify_metadata(data, ROOT / 'vendor')

    def test_old_macro_package_rejected(self):
        data = self.metadata()
        data['packages'].append(dict(name='proc-macro-error', version='1.0.4'))
        with self.assertRaises(ValueError):
            self.check.verify_metadata(data, ROOT / 'vendor')

    def test_lock_checksum_mutation_rejected(self):
        with tempfile.TemporaryDirectory(dir=ROOT / '.work/gtk') as tmp:
            path = Path(tmp) / 'Cargo.lock'
            source = (ROOT / 'Cargo.lock').read_text()
            source = source.replace(self.check.REPLACEMENTS['proc-macro-error3'][1], '0' * 64)
            path.write_text(source)
            with self.assertRaises(ValueError):
                self.check.verify_lock(path)

if __name__ == '__main__':
    unittest.main()


class HarnessLockTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.check = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(cls.check)

    def check_lock(self, packages):
        with tempfile.TemporaryDirectory() as tmp:
            lock = Path(tmp) / 'Cargo.lock'
            lines = ['version = 4']
            for package in packages:
                lines.append('[[package]]')
                lines.extend(f'{key} = {json.dumps(value)}' for key, value in package.items())
            lock.write_text('\n'.join(lines))
            self.check.verify_harness_lock(lock)

    def package(self):
        return dict(name='trybuild', version='1.0.122',
                    source='registry+https://github.com/rust-lang/crates.io-index',
                    checksum='62db9c92d704393fbf2132041720cc80b689f2d3f28521015c2ac866223c11b8')

    def test_maintained_harness_required(self):
        self.check.verify_harness_lock(ROOT / 'vendor/Cargo.lock')

    def test_expected_harness(self):
        self.check_lock([self.package()])

    def test_retired_routes_rejected(self):
        for name in ['trybuild2', 'basic-toml']:
            with self.subTest(name=name), self.assertRaises(ValueError):
                self.check_lock([self.package(), dict(name=name, version='1.0.0')])

    def test_missing_or_duplicate_harness_rejected(self):
        for packages in [[], [self.package(), self.package()]]:
            with self.subTest(packages=packages), self.assertRaises(ValueError):
                self.check_lock(packages)

    def test_substitution_rejected(self):
        for key, value in [('version', '1.0.121'), ('source', 'path+file:///replacement'),
                           ('checksum', '0' * 64)]:
            package = self.package(); package[key] = value
            with self.subTest(key=key), self.assertRaises(ValueError):
                self.check_lock([package])
