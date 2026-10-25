#!/usr/bin/env python3
"""Verify the reviewed GTK backport independently of its descriptive manifest.

Pins below are review inputs, not regenerated from source-manifest.json. Tree
hashes include every archive file (including licenses, original manifests and
VCS metadata). Cargo metadata checks actual resolution from both workspaces.
"""
import argparse
import importlib.util
import hashlib
import json
import os
from pathlib import Path
import subprocess

# Reuse the repository's qualified Python 3.9+ structural-parser fallback.
_policy_spec = importlib.util.spec_from_file_location(
    'abbey_toolchain_policy', Path(__file__).resolve().parents[1] / 'toolchain_policy.py')
_policy = importlib.util.module_from_spec(_policy_spec)
_policy_spec.loader.exec_module(_policy)
tomllib = _policy.tomllib

# archive SHA256, pristine tree SHA256, patched tree SHA256
PINS = {
    'glib-0.18.5': (
        '233daaf6e83ae6a12a52055f568f9d7cf4671dabb78ff9560ab6da230ce00ee5',
        '5da7556cd83c4d07950e32b0bbc212fdf3bd1697f9b2751c3892a33cf25e6c6b',
        '7ccca726775db3ad700271b3a887a7b272bbc024c89296c3e279072223debcf2'),
    'glib-macros-0.18.5': (
        '0bb0228f477c0900c880fd78c8759b95c7636dbd7842707f49e132378aa2acdc',
        'a6c4be94577afb29698c09c95cc17bc28ac87844071da20839150e92e2c4c841',
        '61ed0a651fab6eb6ae0ee516e2086bcaaba7188a93ea7dbe57275f6ec7de98c5'),
    'gtk3-macros-0.18.2': (
        '52ff3c5b21f14f0736fed6dcfc0bfb4225ebf5725f3c0209edeec181e4d73e9d',
        '33b55ab68fe4ef313a22fe1d9d278c10c42fb03118d6356aa6945c4ba29d5e79',
        '919e09b5c9ad26d0d983eb5a3f964afb727805dfffa8a917d6e65f24e5a5ee0e'),
}
REPLACEMENTS = {
    'proc-macro-error3': ('3.1.1', '8f0d4471b3436c22106b21913b1dda531558918ae9b7ec55d58aa84b43552233'),
    'proc-macro-error-attr3': ('3.1.1', '9e564d14133360e1ae169ffde5da25881b5fa47261665b8e5713c212c27799da'),
}
REGISTRY_PROVENANCE = {'proc-macro-error-attr3-3.1.1': {'archive_sha256': '9e564d14133360e1ae169ffde5da25881b5fa47261665b8e5713c212c27799da',
                                  'licenses': {'LICENSE-APACHE': '6fd0f3522047150ca7c1939f02bc4a15662a4741a89bc03ae784eefa18caa299',
                                               'LICENSE-MIT': '544b3aed1fd723d0cadea567affdcfe0431e43e18d997a718f9d67256b814fde'},
                                  'vcs': {'git': {'sha1': '0f0e21bb2a46509c923b87459c6be314bc0cabb5'},
                                          'path_in_vcs': 'proc-macro-error-attr'}},
 'proc-macro-error3-3.1.1': {'archive_sha256': '8f0d4471b3436c22106b21913b1dda531558918ae9b7ec55d58aa84b43552233',
                             'licenses': {'LICENSE-APACHE': '8cdfc2c3fe15f6bdf5926ebac8e983315133b91128d888e6f1bd3227bd0a7cfc',
                                          'LICENSE-MIT': '4d6bf8cef395998be2de2da71eb023ca62603efbd8618b485bb3d1381d011d4b'},
                             'vcs': {'git': {'sha1': '0f0e21bb2a46509c923b87459c6be314bc0cabb5'},
                                     'path_in_vcs': ''}}}
DEV_HARNESS = {'archive_sha256': '62db9c92d704393fbf2132041720cc80b689f2d3f28521015c2ac866223c11b8',
 'license': 'MIT OR Apache-2.0',
 'licenses': {'LICENSE-APACHE': '62c7a1e35f56406896d7aa7ca52d0cc0d272ac022b5d2796e7d6905db8a3636a',
              'LICENSE-MIT': '23f18e03dc49df91622fe2a76176497404e46ced8a715d9d2b67a7446571cca3'},
 'package': 'trybuild',
 'rust_version': '1.88',
 'scope': 'Dev-only compile-test harness; no production dependency',
 'url': 'https://static.crates.io/crates/trybuild/trybuild-1.0.122.crate',
 'vcs': {'git': {'sha1': '3f6a32437cf608d28fb2eda2d8c3e9c0dfc06ff3'}, 'path_in_vcs': ''},
 'version': '1.0.122'}
LICENSE = '8cf56d10131ce201cf69ab74b111d3ebac1acca3833d7efb39ae357224b70edb'


def sha(data):
    return hashlib.sha256(data).hexdigest()


def tree_hash(files):
    return sha(''.join(f'{path}\0{digest}\n' for path, digest in sorted(files.items())).encode())


def require(condition, message):
    if not condition:
        raise ValueError(message)


def verify_sources(vendor):
    require(not vendor.is_symlink(), 'vendor directory symlink forbidden')
    data = json.loads((vendor / 'source-manifest.json').read_text())
    require(data.get('schema') == 1 and set(data['crates']) == set(PINS), 'manifest schema/crates drift')
    require(data.get('registry_replacements') == REGISTRY_PROVENANCE, 'replacement provenance drift')
    require(data.get('dev_harness') == DEV_HARNESS, 'DEV harness provenance drift')
    for crate, (archive, original, patched) in PINS.items():
        entry = data['crates'][crate]
        require(entry['archive_sha256'] == archive, f'{crate}: archive pin drift')
        require(entry['license'] == 'MIT', f'{crate}: declared license drift')
        name, version = crate.rsplit('-', 1)
        require(entry['url'] == f'https://static.crates.io/crates/{name}/{crate}.crate', f'{crate}: source URL drift')
        require(tree_hash(entry['original_files']) == original, f'{crate}: pristine inventory drift')
        require(tree_hash(entry['patched_files']) == patched, f'{crate}: patched inventory drift')
        base = vendor / crate
        require(not base.is_symlink(), f'{crate}: crate directory symlink forbidden')
        paths = list(base.rglob('*'))
        require(not any(p.is_symlink() for p in paths), f'{crate}: symlink forbidden')
        actual = {p.relative_to(base).as_posix(): sha(p.read_bytes()) for p in paths if p.is_file()}
        require(actual == entry['patched_files'], f'{crate}: source bytes/file inventory drift')
        require(actual['LICENSE'] == LICENSE, f'{crate}: license bytes drift')
        require(set(entry['original_files']) == set(actual), f'{crate}: archive file inventory drift')
        changes = sorted(p for p in actual if entry['original_files'][p] != actual[p])
        require(changes == sorted(entry['changed_files']), f'{crate}: changed-file inventory drift')
        for file in ['Cargo.toml', 'Cargo.toml.orig']:
            manifest = tomllib.loads((base / file).read_text())
            require((manifest['package']['name'], manifest['package']['version']) == (name, version), f'{crate}: package identity drift')
            require(manifest['package']['license'] == 'MIT', f'{crate}: license metadata drift')
            if name != 'glib':
                deps = manifest['dependencies']
                require('proc-macro-error' not in deps and 'proc-macro-error3' in deps, f'{crate}: old/aliased macro dependency')
                dep = deps['proc-macro-error3']
                require(dep == {'version': '=3.1.1', 'default-features': False, 'features': ['syn2-error']}, f'{crate}: replacement dependency/feature drift')
    return data


def verify_lock(lock):
    packages = tomllib.loads(lock.read_text())['package']
    require(not any(p['name'] in ('proc-macro-error', 'proc-macro-error-attr', 'proc-macro-error2', 'proc-macro-error-attr2') for p in packages), f'{lock.name}: old macro dependency')
    for name, (version, checksum) in REPLACEMENTS.items():
        matches = [p for p in packages if p['name'] == name]
        require(len(matches) == 1 and matches[0]['version'] == version and matches[0].get('checksum') == checksum, f'{lock.name}: {name} pin drift')
    for crate in PINS:
        name, version = crate.rsplit('-', 1)
        matches = [p for p in packages if p['name'] == name and p['version'] == version]
        require(len(matches) == 1 and 'source' not in matches[0] and 'checksum' not in matches[0], f'{lock.name}: {crate} is not local')


def verify_harness_lock(lock):
    """Pin the reviewed DEV harness and reject the retired vulnerable route."""
    packages = tomllib.loads(lock.read_text()).get('package', [])
    require(not any(p['name'] in ('trybuild2', 'basic-toml') for p in packages),
            'vendor lock: retired compile-test harness route')
    matches = [p for p in packages if p['name'] == 'trybuild']
    require(len(matches) == 1 and matches[0]['version'] == '1.0.122'
            and matches[0].get('source') == 'registry+https://github.com/rust-lang/crates.io-index'
            and matches[0].get('checksum') == '62db9c92d704393fbf2132041720cc80b689f2d3f28521015c2ac866223c11b8',
            'vendor lock: maintained compile-test harness pin drift')


def verify_metadata(metadata, vendor):
    packages = metadata['packages']
    for crate in PINS:
        name, version = crate.rsplit('-', 1)
        matches = [p for p in packages if p['name'] == name and p['version'] == version]
        expected = (vendor / crate / 'Cargo.toml').resolve()
        require(len(matches) == 1 and matches[0]['source'] is None and Path(matches[0]['manifest_path']).resolve() == expected, f'{crate}: resolved source mismatch')
    require(not any(p['name'] in ('proc-macro-error', 'proc-macro-error-attr', 'proc-macro-error2', 'proc-macro-error-attr2') for p in packages), 'old macro package in resolved graph')
    for name, (version, _) in REPLACEMENTS.items():
        matches = [p for p in packages if p['name'] == name]
        require(len(matches) == 1 and matches[0]['version'] == version and matches[0]['source'] == 'registry+https://github.com/rust-lang/crates.io-index', f'{name}: resolved replacement mismatch')
    ids = {p['name']: p['id'] for p in packages if p['name'] in REPLACEMENTS or p['name'] in ('glib-macros', 'gtk3-macros')}
    for parent in ['glib-macros', 'gtk3-macros']:
        nodes = [node for node in metadata['resolve']['nodes'] if node['id'] == ids[parent]]
        require(len(nodes) == 1, f'{parent}: missing/duplicate resolution node')
        require(any(d['name'] == 'proc_macro_error3' and d['pkg'] == ids['proc-macro-error3'] for d in nodes[0]['deps']), f'{parent}: wrong replacement edge')

    nodes = [node for node in metadata['resolve']['nodes'] if node['id'] == ids['proc-macro-error3']]
    require(len(nodes) == 1 and set(nodes[0]['features']) == {'syn2-error'}, 'replacement must enable only syn2-error')
    syn_edges = [edge for edge in nodes[0]['deps'] if edge['name'] in ('syn2', 'syn3')]
    require(len(syn_edges) == 1 and syn_edges[0]['name'] == 'syn2', 'replacement uses wrong Syn API')
    syn = [package for package in packages if package['id'] == syn_edges[0]['pkg']]
    require(len(syn) == 1 and syn[0]['name'] == 'syn' and syn[0]['version'].startswith('2.'), 'replacement Syn 2 edge mismatch')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--sources-only', action='store_true')
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[2]
    vendor = root / 'vendor'
    verify_sources(vendor)
    for lock in [root / 'Cargo.lock', vendor / 'Cargo.lock']:
        verify_lock(lock)
    verify_harness_lock(vendor / 'Cargo.lock')
    if not args.sources_only:
        for manifest, target in [('Cargo.toml', 'consumer'), ('vendor/Cargo.toml', 'vendor')]:
            env = os.environ.copy()
            env.pop('ABI_WDBX_PERSIST', None)
            env.update(CARGO_TARGET_DIR=str(root / '.work/gtk' / f'target-{target}'), CARGO_INCREMENTAL='0', CARGO_PROFILE_DEV_DEBUG='0', CARGO_PROFILE_TEST_DEBUG='0', ABI_WDBX_PATH=':memory:')
            result = subprocess.run([str(root / 'tools/cargo.sh'), 'metadata', '--locked', '--format-version', '1', '--manifest-path', manifest], cwd=root, env=env, stdin=subprocess.DEVNULL, capture_output=True, check=True)
            verify_metadata(json.loads(result.stdout), vendor)
    print('GTK vendor: all three independently pinned source trees, licenses, locks and requested resolution checks passed')


if __name__ == '__main__':
    main()
