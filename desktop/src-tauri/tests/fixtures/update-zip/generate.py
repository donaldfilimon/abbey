#!/usr/bin/env python3
"""Independent ZIP bytes (stdlib) and minisign ED signatures (OpenSSL Ed25519).

Private keys exist only in a disposable directory and are removed on completion.
For native tests --sandbox-root must be the parent's disposable root. All encoded
absolute/drive-relative destinations then stay inside that root even if vulnerable.
"""
import argparse
import base64
import hashlib
import io
import json
import os
import re
from pathlib import Path
import stat
import subprocess
import tempfile
import zipfile


_openssl_candidates = [os.environ.get('ABBEY_ZIP_OPENSSL')]
for _candidate in ('/opt/homebrew/bin/openssl', '/usr/local/bin/openssl', 'openssl'):
    if _candidate not in _openssl_candidates:
        _openssl_candidates.append(_candidate)
OPENSSL = None
for _candidate in _openssl_candidates:
    if not _candidate:
        continue
    try:
        subprocess.check_output([_candidate, 'version'], stderr=subprocess.DEVNULL)
        OPENSSL = _candidate
        break
    except (OSError, subprocess.CalledProcessError):
        continue
if OPENSSL is None:
    OPENSSL = 'openssl'


def run(*args):
    try:
        return subprocess.check_output(args, stderr=subprocess.PIPE)
    except subprocess.CalledProcessError as error:
        raise RuntimeError(error.stderr.decode(errors='replace')) from error


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--sandbox-root', type=Path)
    args = parser.parse_args()
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=True)
    root = args.sandbox_root.resolve() if args.sandbox_root else None
    if root and (out == root or root not in out.parents):
        raise SystemExit('runtime fixtures must be inside sandbox root')
    if root and os.name != 'nt':
        raise SystemExit('absolute Windows fixtures require native Windows')
    if root and not re.fullmatch(r'[A-Za-z]:', root.drive):
        raise SystemExit('runtime fixtures require a local drive sandbox, not UNC/device paths')
    cases = {
        'plain': [('payload.txt', b'harmless updater fixture\n', False)],
        'parent': [('../../outside/parent.txt', b'escape', False)],
        'outward-link': [('outward', b'../../outside', True),
                         ('outward/symlink.txt', b'escape', False)],
        'chained-link': [('first', b'..', True),
                         ('second', b'first/../outside', True),
                         ('second/chained.txt', b'escape', False)],
        'preexisting-link': [('existing/sentinel.txt', b'escape', False)],
    }
    if root:
        cases['absolute'] = [(str(root / 'outside' / 'absolute.txt'), b'escape', False)]
        # The test child sets cwd to outside on this same drive.
        cases['drive-relative'] = [(root.drive + 'relative.txt', b'escape', False)]
    manifest = {'schema': 1, 'generator': 'Python zipfile ZIP_STORED; OpenSSL Ed25519 over BLAKE2b-512 (minisign ED)',
                'version': '99.0.0', 'cases': {}}
    with tempfile.TemporaryDirectory(prefix='throwaway-signing-', dir=out) as scratch:
        scratch = Path(scratch)
        key = scratch / 'test.pem'
        run(OPENSSL, 'genpkey', '-algorithm', 'Ed25519', '-out', str(key))
        public = run(OPENSSL, 'pkey', '-in', str(key), '-pubout', '-outform', 'DER')[-32:]
        keyid = hashlib.sha256(public).digest()[:8]
        pubdoc = b'untrusted comment: disposable updater fixture public key\n' + base64.b64encode(b'Ed' + keyid + public) + b'\n'
        (out / 'updater.pubkey').write_bytes(base64.b64encode(pubdoc) + b'\n')

        def sign(name, data, version=True):
            digest = scratch / 'message'
            digest.write_bytes(hashlib.blake2b(data).digest())
            signature = run(OPENSSL, 'pkeyutl', '-sign', '-rawin', '-inkey', str(key), '-in', str(digest))
            comment = 'file:' + name + ('\tversion:99.0.0' if version else '')
            digest.write_bytes(signature + comment.encode())
            global_signature = run(OPENSSL, 'pkeyutl', '-sign', '-rawin', '-inkey', str(key), '-in', str(digest))
            doc = (b'untrusted comment: disposable test signature\n' + base64.b64encode(b'ED' + keyid + signature)
                   + b'\ntrusted comment: ' + comment.encode() + b'\n' + base64.b64encode(global_signature) + b'\n')
            return base64.b64encode(doc) + b'\n'

        for name, entries in cases.items():
            buffer = io.BytesIO()
            with zipfile.ZipFile(buffer, 'w', compression=zipfile.ZIP_STORED) as archive:
                for path, data, link in entries:
                    info = zipfile.ZipInfo(path, date_time=(2026, 10, 25, 0, 0, 0))
                    info.create_system = 3
                    info.external_attr = ((stat.S_IFLNK | 0o777) if link else (stat.S_IFREG | 0o644)) << 16
                    archive.writestr(info, data)
            payload = buffer.getvalue()
            (out / (name + '.zip')).write_bytes(payload)
            (out / (name + '.zip.sig')).write_bytes(sign(name + '.zip', payload))
            manifest['cases'][name] = {'sha256': hashlib.sha256(payload).hexdigest(), 'entries': [p for p, _, _ in entries]}
        truncated = (out / 'plain.zip').read_bytes()[:40]
        (out / 'truncated.zip').write_bytes(truncated)
        (out / 'truncated.zip.sig').write_bytes(sign('truncated.zip', truncated))
        manifest['cases']['truncated'] = {'sha256': hashlib.sha256(truncated).hexdigest(), 'entries': [], 'construction': 'first 40 bytes of plain ZIP, no central directory'}
        (out / 'plain-unversioned.zip.sig').write_bytes(sign('plain.zip', (out / 'plain.zip').read_bytes(), False))
    manifest['files'] = {p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in sorted(out.iterdir()) if p.is_file() and p.suffix in ('.zip', '.sig', '.pubkey')}
    (out / 'manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')


if __name__ == '__main__':
    main()
