# Updater feed fixtures

Signatures used by `scripts/tests/test_assemble_update_feed.py` to drive
`scripts/assemble-update-feed.sh`. No private key and no real release asset
lives here — only the public `.sig` blobs and the placeholder payload they
were made over.

## Real ones (produced by the installed Tauri CLI)

`@tauri-apps/cli` 2.12.x, run from `desktop/`:

```sh
bun run --silent tauri signer generate -w updater.key -f -p ""   # throwaway key
bun run --silent tauri signer sign -f updater.key -p "" \
    --app-version 2.6.0 "Abbey.app.tar.gz"
```

The throwaway private key was generated in a scratch directory and is not in
this repository. The CLI runs non-interactively with an empty password
(`-p ""`), so no fixture here is hand-written.

| Fixture | Signed file | Signed version | Trusted comment |
| --- | --- | --- | --- |
| `signed-0.1.0-Abbey.app.tar.gz.sig` | `Abbey.app.tar.gz` | `0.1.0` | `timestamp:…\tfile:Abbey.app.tar.gz\tversion:0.1.0` |
| `signed-2.6.0-Abbey.app.tar.gz.sig` | `Abbey.app.tar.gz` | `2.6.0` | `timestamp:…\tfile:Abbey.app.tar.gz\tversion:2.6.0` |
| `signed-2.6.0-Abbey-Personal.app.tar.gz.sig` | `Abbey Personal.app.tar.gz` | `2.6.0` | `timestamp:…\tfile:Abbey Personal.app.tar.gz\tversion:2.6.0` |
| `unversioned-Abbey.app.tar.gz.sig` | `Abbey.app.tar.gz` | — | `timestamp:…\tfile:Abbey.app.tar.gz` |

The personal-edition fixture exists because that asset's file name contains a
space, which is exactly the name the release workflow publishes.

The `unversioned` fixture is what the CLI produces when `--app-version` is
omitted: the trusted comment records the file but no version, which is what
the desktop's `requireSignedVersion` refuses.

## The one synthetic fixture

`signed-no-file.sig` is **hand-built**. The CLI always records the file it
signed, so no real signature can lack the `file:` field, and the assembler's
refusal of such a signature is the only thing this fixture proves. Its
decoded text is byte-faithful to the minisign shape above, but the two inner
base64 lines decode to the literal marker
`SYNTHETIC-Abbey-fixture: not a real minisign signature` instead of an
Ed25519 signature.

The assembler never verifies signatures cryptographically — it has no public
key and never sees one — so this fixture exercises exactly the same code path
a real signature does: base64-decode the `.sig`, read the trusted comment,
bind the feed to it.

## Payload and newlines

Every real fixture was made over the placeholder payload

```
abbey desktop updater feed fixture payload\n
```

which is what the tests write as the fake artifact. The CLI writes its `.sig`
without a trailing newline; these files carry one because the repository keeps
trailing newlines, and the assembler strips whitespace before publishing the
signature (the plugin's base64 decoder rejects whitespace outright).
