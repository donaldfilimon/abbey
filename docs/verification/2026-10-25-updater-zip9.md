# Abbey upstream updater ZIP9 packet — 2026-10-25

This packet removes ZIP4 from Abbey's resolved graph and tests the pinned
updater's actual cached verification API. It does not establish full dependency
security, a native Windows installer update, or integrated product qualification.
The production updater declaration, endpoints, key, capabilities and app code
are unchanged.

## Source and dependency identity

- Base: `3b931284e592cb832dcd584a68d5553a9d1e3581`; compatible predecessor
  `8e266b7`. Adjacent read-only ABI: `4b0821660387f059b86d6da6a168acbf58257538`;
  WDBX: `81ae29a7c472d42765fd1f09f75c1288b0252d30` (both clean during checks).
- Updater 2.14.0 is patched to
  `git+https://github.com/tauri-apps/plugins-workspace?rev=ba98bfadc4c868c9a7e1a226c83af528e1f11b89#ba98bfadc4c868c9a7e1a226c83af528e1f11b89`.
  Locked metadata discovered the checkout; Git HEAD matches, and tracked updater
  source has no diff. The reviewed commit changes its Windows ZIP requirement
  from 4 to 9; upstream notes the changed `Error::Extract` inner error type.
- ZIP is solely **9.0.5**, registry checksum
  `6e04976da629d3b04204ba8105b396a193af8ad2583b2c79bfb3ab8af580a11e`.
  ZIP has no enabled optional Cargo features; fixtures therefore use ZIP_STORED.
  Updater features are `default,rustls-tls,system-proxy,zip`. Both the actual
  updater and the explicit test contract resolve this same ZIP package.
- Tokio 1.54.1 and ctutils 0.4.4 are preserved. Cargo's unrelated windows-sys
  edge rewrites were restored to their baseline values, then accepted by full
  locked metadata. ZIP4-only arbitrary/derive_arbitrary disappear.
- Updater requires Rust 1.90 and is Apache-2.0 OR MIT; ZIP is MIT. Actual
  qualification compiler is Rust 1.99.1 stable, aarch64-apple-darwin. Upstream
  license/source hashes and the final lock hash are in
  [source-inventory.json](2026-10-25-updater-zip9/source-inventory.json).

## Executed checks and boundaries

Every check's exact raw command/environment/time/exit/source receipt and command
output is retained privately under ignored `.work/zip9/private-evidence` and
`.work/zip9/raw-evidence`. [checks-summary.json](2026-10-25-updater-zip9/checks-summary.json)
is an explicitly portable published summary: it retains arguments, selected
setting labels, UTC times, exits, source/lock SHA-256 values, disk availability,
raw receipt/output hashes and private relative references. `$REPO`,
`$UPDATER_CHECKOUT` and `$RESOLVED_EXECUTABLE` are publication substitutions;
they are not the raw runtime values or configured dependency paths. Originals
were copied intact before publication and are not normalized or deleted.
Later raw checks also record adjacent source identities/status and expanded root
source hashes. The recording helper now writes fresh raw receipts only under
ignored `.work/zip9/receipts`; the inventory verifier reads freshly recorded
locked metadata there. No machine-specific cache/toolchain paths are published.

| Check | Result | Interpretation |
| --- | --- | --- |
| Baseline dependency scan | exit 1 | Dependency RED: ZIP4 advisories RUSTSEC-2026-0367/0368/0369 plus existing GLib/proc-macro-error warnings |
| Locked metadata, updater/ZIP inverse trees, source clean/head, inventory | exit 0 | Exact reviewed Git source and only ZIP9; compatible pins and fixture hashes verified |
| Safe public cached/signature suites | 5 cached + 3 signature cases pass | Existing optional built-artifact case returns early; only 2 signature checks exercised |
| Personal public cached/signature suites | same | Edition cohort independently compiled/tested |
| Portable harness and ZIP dependency contract, safe/personal | 2 portable tests + 1 Unix contract pass per cohort | Independent fixture generation/manifest hashes/actual plugin signatures, bounded child success/termination, outside sentinel snapshot, exact ZIP9 pre-existing Unix symlink refusal |
| Feed assembly Python suite | 19 pass | Version/name/signature binding and malformed/missing release inputs |
| Package-scoped rustfmt | pass | Includes Windows-only Rust syntax parsing, not Windows type-checking |
| Final dependency scan | exit 1 | ZIP advisories gone; RUSTSEC-2024-0370 proc-macro-error and RUSTSEC-2024-0429 GLib remain denied; no ignores added |

One actual harness RED is preserved: `portable-safe` failed because Cargo's PATH
selected Apple's OpenSSL without working Ed25519 generation. `portable-safe-green`
passes after resolving the supported OpenSSL path before the wrapper and passing
`ABBEY_ZIP_OPENSSL`. This is harness portability evidence, not a fabricated
Windows security RED. Application implementation did not change; dependency
RED is the baseline advisory scan.

The public API tests configure the real plugin using a Tauri mock runtime, obtain
`UpdaterBuilder` through `UpdaterExt`, restore a cached release, and call
`Update::verify`. They cover valid bytes, altered bytes, malformed signature/key,
a well-formed signature over different bytes, announced-version mismatch and
missing required signed version. Only altered bytes reach `install`; its exact
signature error must precede platform changes, and the prior executable/scratch
directory remain unchanged. A client hook panics on any request attempt.

Native hostile archives carry valid test signatures and use the actual plugin's
`install` path. They contain no installer entries. Each case has its own bounded
child and TEMP/TMP/cwd beneath one disposable root, with unchanged outside
sentinels and exact ZIP extraction refusal required. The separately named
`updater_zip_contract` directly invokes ZIP9 for deterministic pre-existing
symlink/junction behavior; it is not updater extraction evidence. No private
plugin function or replacement extractor is copied or patched.

## Native Windows operation still required

This is a macOS ARM64 host. The Rust x86_64-pc-windows-msvc standard-library
target is installed. Per the controller ruling, the full cohort cross-build was
not attempted within this packet's disk budget. No native
Windows execution was available. Windows-only Rust bodies are **uncompiled and
unexecuted**. PowerShell syntax/execution is **unexecuted** because no parser is
installed. macOS's absent Windows cases and the ignored child dispatcher are not
Windows passes. Only platform-independent helpers and the Unix symlink contract
actually ran.

| Platform/workflow | Status |
| --- | --- |
| macOS ARM64 cached verification and portable fixture/helper/library contracts | executed as above |
| Windows 11 ARM64 MSVC hostile ZIP, symlink/junction and cached suites | UNEXECUTED |
| Windows x64 MSVC hostile ZIP, symlink/junction and cached suites | UNEXECUTED |
| Other supported native Windows target rows | UNEXECUTED; record separately |
| Real safe/personal NSIS/MSI ZIP update, restart/version, edition state and failed-update recovery | UNEXECUTED operator VM acceptance |
| POSIX SUID/SGID/chmod contract, Windows ACL, same-user plugin temp-directory race | not exercised by this packet |

On a disposable native Windows VM with adjacent pinned repositories, Rustup,
Python 3, supported OpenSSL, PowerShell 7.2+, MSVC build tools and link privilege:

```powershell
pwsh -NoProfile -File desktop/scripts/qualify-updater-windows.ps1
```

The runner resolves `rustup which --toolchain 1.99.1 cargo/rustc`, verifies the
compiler release and matching native MSVC OS architecture, creates private
build/temp roots under `.work`, closes stdin, records each command and its source
hashes, and retains outputs and per-case evidence. Missing link privilege is an
explicit failing UNEXECUTED setup condition. It runs:

```text
test --locked -p abbey-desktop --test updater_windows_zip -- --test-threads=1
test --locked -p abbey-desktop --features personal-edition --test updater_windows_zip -- --test-threads=1
test --locked -p abbey-desktop --test updater_zip_contract -- --test-threads=1
test --locked -p abbey-desktop --test updater_signature --test updater_cached_verification
test --locked -p abbey-desktop --features personal-edition --test updater_signature --test updater_cached_verification
```

Default tests never install a valid archive. Positive acceptance is an explicit
separate mode, requiring the operator's local VM-specific UI/installer adapter:

```powershell
pwsh -NoProfile -File desktop/scripts/qualify-updater-windows.ps1 `
  -Mode InstallerAcceptance -DisposableVm -AcceptanceScript .work/local-vm-acceptance.ps1
```

The adapter receives `-EvidenceRoot`, runs only disposable installations with
local test-signed feed/packages and test keys, and writes `acceptance.json`.
The adapter must record native OS/architecture, edition, installer type,
source/bundle/signature hashes, before/after/restart version, edition namespace
and representative state hashes, local feed identity, and each refusal scenario
with proof the prior installed application remains runnable and unchanged.
Run safe and personal NSIS/MSI rows independently, including a corrupted package,
version mismatch, missing signed version, extraction refusal and interrupted
update/recovery. Record actions and observed outputs; a script exit alone does
not qualify these rows. The runner retains the adapter and its command output
but does not invent the VM-specific UI automation or positive acceptance result.

## Remaining gates and recovery

`./check.sh` and `./desktop/check.sh` are explicitly deferred to the integrated
GTK-repaired cohort. Existing WDBX `AdmissionSimulator` unused-import warnings
remain visible in focused compiler output. Full security and desktop/platform
qualification are pending. No release publication, production credentials,
installed app changes, original checkout edits or cache source edits occurred.
The independent APFS-cloned `.work/zip9/target` was used, with incremental and
dev/test debug disabled, scratch in-memory WDBX and stdin EOF. Disk remained
above 10 GiB. Prior candidate/cache receipts remain intact. Reverting this
packet restores the original plugin/lock; no live state needs rollback.
