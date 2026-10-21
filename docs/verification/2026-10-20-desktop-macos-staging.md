# 2026-10-20 — desktop macOS staging: bundling, updater wiring, hardening gates

## What this is

Evidence for the `desktop/` Tauri workspace reaching "ready for signing and
publish": the first real bundled `.app`/`.dmg` (every prior acceptance used
`tauri build --debug --no-bundle`), the bundled hardening metadata, a first-run
staging ad-hoc signature, the Tauri updater wired end to end on the build side,
and the CI surface that keeps continuing to prove it. All of it runs **without
any Apple credential** — no Developer ID identity exists on this machine
(`security find-identity -p codesigning` → 0 valid identities, `notarytool` has
no stored credentials), so signing/notarization remain owner steps and are
*not* claimed here.

## First-ever bundled artifacts (release profile)

`./scripts/release-macos.sh` (staging mode, no environment) produced and
verified, at `desktop/target/release/bundle/`:

```
Finished 2 bundles at:
    .../bundle/macos/Abbey.app (16.93 MiB)
    .../bundle/dmg/Abbey_2.6.0_aarch64.dmg (5.11 MiB)
    .../bundle/macos/Abbey.app.tar.gz (updater) (5.28 MiB)
Finished 1 updater signature at:
    .../bundle/macos/Abbey.app.tar.gz.sig
```

- `codesign --verify --strict` → `valid on disk`, `satisfies its Designated
  Requirement`.
- `codesign -dv` → `flags=0x10002(adhoc,runtime)`, `TeamIdentifier=not set`
  (staging) — hardened runtime is on, and the signature is the script's own
  deterministic ad-hoc signature (`--force -s - --deep --options runtime`),
  because a *linker-signed* Mach-O inside an `.app` (no `_CodeSignature`,
  `Sealed Resources=none`) is not a signed bundle; the script signs by mode,
  never by probing for a signature.
- `codesign -d --entitlements ...` → exactly `com.apple.security.cs.allow-jit =
  true` (WKWebView JIT under hardened runtime; the minimal entitlement, no
  sandbox, no host authority).
- `SHA256SUMS.txt` written next to the artifacts (dmg + bundle binary + updater
  tarball + updater `.sig`).

## First-run bundling fallout, fixed in-repo

1. **Workspace target path**: the bundler writes to the *workspace* dir
   `desktop/target/release/bundle/`, not `src-tauri/target/` (src-tauri is a
   member of the desktop workspace). Release script + packaged prover corrected.
2. **DMG detach race**: `bundle_dmg.sh` detaches its staging image on a timer;
   a Finder/Spotlight worker grabbing the fresh volume made `hdiutil detach`
   fail with `Resource busy` (twice). The release script now ejects any stuck
   `/Volumes/dmg.*` interstitial and retries up to 3 times (bounded; third
   attempt failed-fast with the log preserved). Observed: attempt 1 failed,
   attempt 2 passed.
3. **rust ↔ npm version alignment**: adding `tauri-plugin-updater` moved the
   Rust `tauri` crate to 2.12.4 (and `wry` 0.55.1 → 0.57.0); the CLI then
   refused to run with `@tauri-apps/api` 2.11.1. npm packages aligned to
   `@tauri-apps/api 2.12.4` + `@tauri-apps/cli 2.12.3` (no 2.12.4 CLI exists;
   patch offsets are accepted by the CLI's same-minor check). This is the ONE
   dependency-family bump the updater work caused and it is recorded here on
   purpose: wry 0.57.0 is now exercised by the packaged acceptance, not assumed.

## Updater configured and triggered (build side complete, user-initiated trigger)

- `tauri-plugin-updater 2.14.0` registered in `src/main.rs`; capability grants
  exactly `["core:default", "updater:default"]` — `updater:default` is the one
  deliberate execution-adjacent permission, documented in the capability file
  and enforced by an updated `no_execution_capable_plugin_is_declared` test.
- `tauri.conf.json`: `bundle.createUpdaterArtifacts: true`, `plugins.updater`
  with the pinned Ed25519 `pubkey` and the GitHub-Releases `latest.json`
  endpoint (`https://github.com/donaldfilimon/abbey/releases/latest/download/latest.json`).
- **Version binding (2026-10-21).** `plugins.updater.requireSignedVersion` is
  now `true`. The plugin's own verification path
  (`tauri-plugin-updater-2.14.0/src/updater.rs::verify_signature`) base64-decodes
  the feed's `signature` field into the minisign signature text, verifies it
  against the pinned public key, and then requires the version announced by the
  endpoint to equal the version inside the signature's **trusted comment** —
  without that, a crafted endpoint response could pair an inflated version with
  an older, validly signed artifact. The Tauri bundler already records
  `version:` in the trusted comment (`...file:Abbey.app.tar.gz\tversion:2.6.0`),
  so the binding is enforceable; the feed assembler derives the published
  version from that signed comment and fails closed on a mismatch.
- **The pinned key verifies real artifacts (2026-10-21).**
  `desktop/src-tauri/tests/updater_signature.rs` runs the plugin's *own*
  verification path with the same `minisign-verify` crate the plugin uses:
  1. the key pinned in `tauri.conf.json` parses as a real minisign public key;
  2. a committed fixture (a genuine Tauri-CLI signature over a stand-in
     artifact, produced with a throwaway key that was deleted) verifies, and its
     trusted comment carries the `file:`/`version:` binding the plugin reads;
  3. with `ABBEY_DESKTOP_UPDATE_ARTIFACT` set, the **built** artifact verifies
     against the **shipped** pin. Both editions' staging artifacts passed:
     `verified ../target/release/bundle/macos/Abbey.app.tar.gz against the
     shipped updater pin (signed version 2.6.0)` and the same for
     `Abbey Personal.app.tar.gz`.
  The release workflow now runs (3) against each edition's artifact before it
  assembles a feed, so a release cannot publish an artifact the shipped client
  would reject.
- Staging key ceremony: `scripts/updater-keygen.sh` writes the private key to
  `~/.local/share/abbey-updater/` (outside this repository, 0600) and prints
  the pubkey for embedding. The current committed pubkey is the **staging**
  key generated on this machine; swapping to a production key is a documented
  re-embed before the first public release.
- Build output above shows the updater artifact pipeline signing (`Abbey.app.tar.gz.sig`,
  420 bytes) on every staging build.
- Deliberate trigger policy: the Doctor surface's "Updates" card
  (`desktop/src/components/UpdateCheck.tsx`) is the single trigger — checking is
  user-initiated, the plugin verifies the artifact against the pinned key before
  install, and without the process plugin there is no in-app relaunch (the
  surface says "quit and reopen to finish"). Nothing is automatic. The safe
  edition checks `latest.json`; the personal edition's overlay endpoint
  (`tauri.personal.conf.json`) checks `latest-personal.json` — per-edition
  feeds, one shared pinned key. Feed assembly ships as
  `desktop/scripts/assemble-update-feed.sh` (+ `scripts/tests/test_assemble_update_feed.py`)
  for the release pipeline.

## Surface polish: menu bar, icon, window defaults

- **Menu bar** (`src/main.rs` `.menu(...)` hook): Apple-standard predefined
  items only — app menu (About with the app version from the same config the
  doctor surface prints, hide/quit), Edit (undo/redo/cut/copy/paste/select-all)
  so the composer gets standard keyboard shortcuts, Window
  (fullscreen/minimize/zoom/bring-all-to-front/close). No custom handlers: menu
  entries cannot reach webview state, the daemon, or the filesystem.
- **Icon**: the tracked icon was a flat placeholder rectangle. Replaced by a
  1024-px master rendered from vector geometry in this repo
  (`desktop/scripts/render-icon.swift`: gradient squircle + white Romanesque
  abbey-arch doorway opening onto depth), with the full chain regenerated by
  `bun run tauri icon` (icns verified to contain 16…512@2x levels; Android/iOS
  noise pruned). The master is tracked so the chain is reproducible.
- **Window defaults** (`tauri.conf.json`): `center: true` added on top of the
  existing title/size/min-size so first launch is centered without code, and the
  title is now set at startup from the edition's product name (`main.rs`
  `setup`) — the personal build no longer presents a title bar reading "Abbey".
- **Favicon**: `desktop/public/favicon.png` (from the regenerated chain) plus a
  `<link rel="icon">` in `index.html`, so the webview shows the same mark as
  the bundle.
- **Packaged proof (2026-10-21).** `desktop/scripts/prove-menu-icon-macos.sh`
  drives the **packaged** app's Accessibility tree and inspects the bundle's
  resources; it never substitutes the unpackaged build. Both editions pass
  **10/10**:
  | Check | Safe edition | Personal edition |
  | --- | --- | --- |
  | AX menu bar (top level) | `Apple │ Abbey │ Edit │ Window` | `Apple │ Abbey Personal │ Edit │ Window` |
  | AX app / Edit / Window menu items | present | present |
  | AX window title | `Abbey` | `Abbey Personal` |
  | bundle `icon.icns` vs tracked chain | sha256 identical | sha256 identical |
  | tracked master | 1024×1024 | 1024×1024 |
  | icns 1024 level | present (12 entries) | present (12 entries) |
  | placeholder assets in the bundle | none | none |
  The receipts are `desktop/target/menu-acceptance/menu-receipt-safe.json` and
  `menu-receipt-personal.json`. The OS's own labels are recorded verbatim in
  the receipt (the Window menu reads `minimize`, `zoom`, `toggle full screen`,
  `close window`), and the prover accepts a documented set of label spellings
  per item rather than one hardcoded string.
  A run that checks nothing can never report success: an aborted run (unknown
  argument, missing bundle) exits non-zero with `status: failed` and a
  `runner_completed` failed check in the receipt.
  Gate wiring: `ABBEY_DESKTOP_MENU_ACCEPTANCE=1 ./desktop/check.sh` runs it as
  a required step, and the script's syntax is checked in every desktop gate.


## Gates

- `desktop/check.sh` → full pass: native assertion tests, codegen drift,
  script-syntax, typecheck, model-state recovery tests, frontend build,
  `verify-bundle: OK (3 built files)`, cargo tests 31/31 in the default **and**
  `personal-edition` modes, live scratch-`abbeyd` daemon read proof 3/3,
  no-silent-fallback 1/1, real link build.
- Workflow guards: `tools/tests/test_workflow_guards.py` 38/38 OK, including
  the new `DesktopWorkflowGuards` (14 tests) over `.github/workflows/desktop.yml`.
- `desktop/codegen -- --check` → `5 generated file(s) up to date` against the
  rewritten `tauri.conf.json` (the edition patcher preserves every key except
  `identifier`/`productName`).

## CI

`.github/workflows/desktop.yml` adds `gate-desktop-macos` on the same
self-hosted macOS ARM64 runner and trusted-events boundary as `rust.yml`:

- pinned checkout + pinned toolchain;
- the two public sibling checkouts at the same immutable pins (unit cross-checked
  against rust.yml by the new guard tests);
- pinned bun (env `BUN_VERSION: 1.4.4`, asserted after install);
- `./desktop/check.sh` unconditionally;
- staged release smoke: ephemeral per-run updater key on `RUNNER_TEMP`,
  `release-macos.sh --skip-dmg --updater-key …` (DMG is left to local/owner
  steps so a headless runner cannot fail the gate on the hdiutil race);
- `Clean runner workspace` step (always) removing all three checkouts.

`.github/workflows/desktop-release.yml` (authored, not executed — it needs the
owner secrets; see the checklist below) is the publish path:

- triggers: tag push `abbey-desktop-v*` + `workflow_dispatch` only — no
  pull_request trigger of any kind, so fork pull requests can never reach it;
- `contents: write` is the one deliberate permission exemption, scoped to the
  release job's purpose (creating the draft release); still trusted-events
  only;
- same runner pool, gate (`vars.ABBEY_MACOS_ARM64_RUNNER == 'enabled'` +
  repository equality), and the same checkout/toolchain/sibling/bun pins as
  `desktop.yml`;
- fail-closed secret guard first: signing identity, App Store Connect API key
  (base64 `.p8`), key ID/issuer, and the production updater key must all be
  present or the job stops with an actionable message; key material is
  materialized under `${RUNNER_TEMP}/abbey-release-secrets` mode 600 and never
  echoed;
- `desktop/check.sh`, then **both editions** built with DMGs and the production
  updater key, per-edition `SHA256SUMS` snapshotted before the second build
  reuses the shared filename, tag-version asserted against each bundle's
  `CFBundleShortVersionString` via PlistBuddy;
- feeds assembled per edition with `desktop/scripts/assemble-update-feed.sh`
  (safe → `latest.json`, personal → `latest-personal.json`) and published as
  release assets alongside both DMGs/tars/sigs/checksums — this is what makes
  the per-edition updater endpoints actually serve;
- draft `gh release create` (hard-fails if `gh` is absent — no brew fallback),
  then the always-runner cleanup step.

Guard coverage: `tools/tests/test_workflow_guards.py` grew
`DesktopReleaseWorkflowGuards` (14 tests) — no pull_request strings, the
documented `contents: write` exemption, RUNNER_TEMP-only key material, feed
invocations, `--draft`, cleanup. 59/59 total OK.

## Personal-edition staging parity

`./scripts/release-macos.sh --personal` built the edition overlay end to end:

```
Abbey Personal.app (16.92 MiB)
Abbey Personal_2.6.0_aarch64.dmg (5.11 MiB)
Abbey Personal.app.tar.gz (updater) + .sig
```

- `codesign --verify --strict` → valid; `flags=0x10002(adhoc,runtime)`;
  `Identifier=com.donaldfilimon.abbey-personal`; packaged `Info.plist` carries
  `CFBundleIdentifier=com.donaldfilimon.abbey-personal` /
  `CFBundleName=Abbey Personal` — the safe edition's bundle cannot be
  installed over, and vice versa (this is the runtime half of
  `edition-compile-time-separation`).
- Bug found and fixed by this run: the release script picked the updater
  tarball by glob order, so for `--personal` builds a stale safe-edition
  `Abbey.app.tar.gz` could ride into the personal SHA256SUMS. The script now
  matches `$(basename "$APP").tar.gz` exactly.
- Open design item unchanged: one shared updater endpoint cannot serve two
  editions' `latest.json` (the platform key holds one artifact set); per-edition
  publish paths are deferred to the releases-workflow task alongside the
  production key ceremony.

## Owner steps that cannot be done here (credentials)

1. Create the updater **production** keypair (`./scripts/updater-keygen.sh`),
   replace `plugins.updater.pubkey` in `tauri.conf.json`, and discard the
   staging key documented above.
2. `APPLE_SIGNING_IDENTITY` = "Developer ID Application: …" identity (none
   exists on this machine).
3. One notarization set: `APPLE_API_KEY_PATH`/`APPLE_API_KEY_ID`/`APPLE_API_ISSUER`
   **or** `APPLE_ID`/`APPLE_PASSWORD`/`APPLE_TEAM_ID`.
4. First signed, notarized run:
   `APPLE_SIGNING_IDENTITY="…" APPLE_API_KEY_PATH=… APPLE_API_KEY_ID=… APPLE_API_ISSUER=… \
    ./scripts/release-macos.sh`
   — bundler signs + notarizes; the script then runs `spctl -a -vv` and
   `stapler validate` on the app (+ dmg) and writes SHA256SUMS. Gatekeeper
   rejects fresh downloads of a signed-but-unnotarized bundle, so the
   notarization env is required for a public artifact.

## Packaged-app Routes acceptance

`./scripts/prove-packaged-macos.sh --skip-build` (`desktop/scripts/prove_packaged_macos.py`,
subclassing the canonical unpackaged prover): the same scratch-`abbeyd` setup,
same native Accessibility driver, and the exact same scenario table, launched
at the bundle-internal binary `Abbey.app/Contents/MacOS/abbey-desktop` instead
of the debug developer binary.

  PASS authenticated-25 · limit-10 · limit-50 · authentication-rejection ·
       daemon-loss-no-fallback · empty-log · route-files-unchanged

- Receipt `desktop/target/routes-acceptance/packaged-receipt.json`: `status: passed`,
  24 native snapshots, 1 discarded traversal; `codesign` record shows
  `flags=0x10002(adhoc,runtime)` on the packaged binary
  (`sha256 e0ad799f0ac21d574d9289ba32518c4f6c2f3c0c2defc9350a8f37ff6657c1a3`).
- The first attempt failed on `limit-10` timing out, while the second passed
  all seven scenarios within 10 s with identical binaries — consistent with an
  AX/viewport race against the frontend's poll loop, not a packaged difference;
  recorded rather than hidden.
- Boundary, deliberately: this proves bundle-layout + packaged-Info.plist +
  staged (ad-hoc) signature acceptance. NOTARIZED first-launch behavior
  (quarantine, Gatekeeper, `open` execution) needs the owner's Developer ID
  credentials and remains an owner step.

