"""Run at the Abbey root after recorded locked metadata; fail closed on graph drift."""
import hashlib
import json
from pathlib import Path
import subprocess
import tomllib

out = Path('docs/verification/2026-10-25-updater-zip9')
metadata_path = Path('.work/zip9/receipts/metadata-qualified.stdout')
if metadata_path.exists():
    metadata = json.loads(metadata_path.read_text())
else:
    import gzip
    metadata = json.loads(gzip.decompress(Path('.work/zip9/private-evidence/metadata-qualified.stdout.gz').read_bytes()))
packages = metadata['packages']
updater, = [p for p in packages if p['name'] == 'tauri-plugin-updater']
zip9, = [p for p in packages if p['name'] == 'zip']
rev = 'ba98bfadc4c868c9a7e1a226c83af528e1f11b89'
assert updater['source'] == f'git+https://github.com/tauri-apps/plugins-workspace?rev={rev}#{rev}'
assert zip9['version'] == '9.0.5'
lock = tomllib.loads(Path('Cargo.lock').read_text())
locked_zip, = [p for p in lock['package'] if p['name'] == 'zip']
assert locked_zip['version'] == '9.0.5'
assert next(p for p in lock['package'] if p['name'] == 'tokio')['version'] == '1.54.1'
assert next(p for p in lock['package'] if p['name'] == 'ctutils')['version'] == '0.4.4'
source = Path(updater['manifest_path']).parent
repo = source.parents[1]
assert subprocess.check_output(['git', '-C', str(repo), 'rev-parse', 'HEAD'], text=True).strip() == rev
subprocess.run(['git', '-C', str(repo), 'diff', '--exit-code', 'HEAD', '--', 'plugins/updater'], check=True)
paths = subprocess.check_output(['git', '-C', str(repo), 'ls-files', 'plugins/updater', 'LICENSE_APACHE-2.0', 'LICENSE_MIT', 'Cargo.toml'], text=True).splitlines()
source_hashes = {p: hashlib.sha256((repo / p).read_bytes()).hexdigest() for p in paths if (repo / p).is_file()}
nodes = {n['id']: n for n in metadata['resolve']['nodes']}
result = {
    'updater': {'id': updater['id'], 'source': updater['source'], 'version': updater['version'], 'license': updater['license'], 'rust_version': updater['rust_version'], 'features': nodes[updater['id']]['features'], 'tracked_source_sha256': source_hashes},
    'zip': {'id': zip9['id'], 'license': zip9['license'], 'rust_version': zip9['rust_version'], 'checksum': locked_zip['checksum'], 'features': nodes[zip9['id']]['features']},
    'lock_sha256': hashlib.sha256(Path('Cargo.lock').read_bytes()).hexdigest(),
    'pins_preserved': {'tokio': '1.54.1', 'ctutils': '0.4.4'},
    'windows_updater_manifest_requirement': 'zip version 9; optional; default-features=false; updater default zip feature enabled',
}
(out / 'source-inventory.json').write_text(json.dumps(result, indent=2) + '\n')
fixtures = Path('desktop/src-tauri/tests/fixtures/update-zip')
manifest = json.loads((fixtures / 'manifest.json').read_text())
for name, digest in manifest['files'].items():
    assert hashlib.sha256((fixtures / name).read_bytes()).hexdigest() == digest, name
import zipfile
for archive in fixtures.glob('*.zip'):
    if archive.stem == 'truncated':
        continue
    with zipfile.ZipFile(archive) as z:
        assert all(not name.lower().endswith(('.exe', '.msi')) for name in z.namelist())
print('Exact Git source clean; only ZIP9.0.5; compatible pins preserved; fixture hashes/no installers verified.')
print('ZIP checksum:', locked_zip['checksum'])
print('ZIP features:', ', '.join(nodes[zip9['id']]['features']))
