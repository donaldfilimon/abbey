# Updater signature fixture

A real Tauri/minisign signature used by `desktop/src-tauri/tests/updater_signature.rs`
to prove that the desktop client's updater verification path actually works, rather
than only asserting that a pinned key is *shaped* like a key.

| File | What it is |
| --- | --- |
| `Abbey.app.tar.gz` | the signed payload (40 bytes; a stand-in for an updater artifact) |
| `Abbey.app.tar.gz.sig` | the Tauri bundler's `.sig` file — base64 of the minisign signature text, byte-identical in encoding to what `release-macos.sh` produces next to a real `.app.tar.gz` |
| `updater.pubkey` | the matching public key file, in the same encoding `plugins.updater.pubkey` uses in `tauri.conf.json` (base64 of the minisign public key text) |

## Provenance

Signed with the real Tauri CLI on 2026-10-21:

```
bun run tauri signer generate -w <scratch>/test.key -f -p ""
bun run tauri signer sign -f <scratch>/test.key -p "" --app-version 2.6.0 Abbey.app.tar.gz
```

The signing key was a **throwaway** created in a scratch directory outside this
repository and deleted immediately afterwards; only the public key and the
signature are committed. It is deliberately **not** the key pinned in
`tauri.conf.json` — that pin is the staging/release key and is swapped for the
production key before the first public release.

The signature's trusted comment reads `timestamp:…	file:Abbey.app.tar.gz	version:2.6.0`,
which is the same field layout `tauri-plugin-updater` reads to bind an announced
version to a signed artifact (`requireSignedVersion`).
