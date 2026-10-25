# Abbey updater ZIP9 compatibility and qualification

**Goal:** Remove the actual Windows updater ZIP4 dependency while retaining signature/edition/recovery behavior and adding executable hostile-archive fixtures.
**Architecture:** Pin the exact reviewed upstream updater revision; test its actual public API. No duplicate extraction engine.
**Specification:** Approved modernization M2 baseline dependency repair, M11 supported desktop execution and M14 update/rollback; Abbey dependency-security-remediation and desktop-platform acceptance remain distinct.
**Technology:** Rust1.99.1, Tauri updater2.14.0 at exact Git revision, ZIP9.0.5, existing test fixtures and a native Windows PowerShell harness.

## Global Constraints

- Preserve existing canonical specifications, original source snapshots, dirty/ignored/unpushed work, historical candidates and receipts. Existing paths and package identities remain compatible unless a malformed-input refusal is explicitly specified.
- No security ignores, fake package versions, live deployment, production signing keys, installed application changes or external messages. Run only isolated fixtures and scratch stores.
- Use the exact task checkout with isolated writable build output; ABI/Abbey use their wrapper. Leave ABI_WDBX_PERSIST unset with ABI_WDBX_PATH=:memory: fallback. Stdin is closed.
- The reviewed compatible-pin cohort already has Tokio1.54.1 and ctutils0.4.4. Preserve those pins and unrelated dependency edges; use real locked metadata/audits, not copied replacement lockfiles.
- Existing GTK/macro and other remaining warnings stay visible. This ZIP packet cannot establish full cohort security, platform or live-update qualification.
- Follow regression-first development for changed application behavior; retain scanner failure as baseline dependency evidence. Platform-unavailable tests remain explicitly unexecuted, never counted as passing.
- CARGO_INCREMENTAL=0 and CARGO_PROFILE_DEV_DEBUG/CARGO_PROFILE_TEST_DEBUG=0 bound local compiler output. Existing completed-task caches may be independently cloned with provenance, never shared writable directories or hardlinks. Check disk before heavy builds and report below10GiB.

## Task 1: Abbey: pin and qualify the upstream ZIP 9 updater

### Ownership

- `Cargo.toml`: root `[patch.crates-io]` entry for the exact reviewed updater revision.
- `Cargo.lock`: this task's resolved updater/ZIP graph, integrated with the lockfile owner.
- `desktop/src-tauri/Cargo.toml`: only test dependencies/features required to exercise the actual public updater API; retain the existing production updater declaration unless resolution requires a reviewed change. A test-only Tauri `test` feature may be needed for `tauri::test::mock_app`.
- Existing `desktop/src-tauri/tests/updater_signature.rs`: retain the real signed fixture and shipped-key assertions; clarify evidence boundaries if comments change.
- New `desktop/src-tauri/tests/updater_cached_verification.rs`: actual pinned `UpdaterBuilder` / `restore_update` / `Update::verify` integration, with a client hook that fails on any network attempt.
- New `desktop/src-tauri/tests/updater_windows_zip.rs`: Windows-only, parent/child-isolated hostile archive extraction tests through the real pinned plugin. Parent owns child termination, temporary paths and evidence.
- New `desktop/src-tauri/tests/fixtures/update-zip/{README.md,manifest.json,...}`: small independently generated ZIP bytes, test-only public keys/signatures, generation provenance and SHA-256 values. Keep existing `fixtures/update-signature/` immutable unless an independently justified fixture change is necessary.
- New `desktop/scripts/qualify-updater-windows.ps1`: reproducible native Windows test/build runner and receipt collection. Do not run a valid installer as part of ordinary unit tests.
- New `docs/verification/2026-10-25-updater-zip9.md` and associated evidence directory. Any broader README/claims/generated-ledger changes remain with the documentation/integration owner.

No application updater implementation change is expected. `desktop/src-tauri/src/main.rs`, updater endpoint/key configuration, capability permissions, code generation and release publication remain outside this task unless a focused regression demonstrates a required adjustment and ownership is coordinated.

### Exact upstream identity and review

The preserved candidate pins:

```toml
[patch.crates-io]
tauri-plugin-updater = { git = "https://github.com/tauri-apps/plugins-workspace", rev = "ba98bfadc4c868c9a7e1a226c83af528e1f11b89" }
```

Local inspection verified this commit identity and its subject, `chore(deps): update rust crate zip to v9 (#3698)`. Its updater package remains 2.14.0; `plugins/updater/Cargo.toml` changes the Windows-only ZIP requirement from `4` to `9`, retaining `default-features = false` and optional activation. `.changes/updater-zip-9.md` explicitly notes that Windows `Error::Extract` now wraps ZIP 9's `ZipError`. The upstream workspace requires Rust 1.90, below the product's pinned Rust 1.99.1. Licenses are Apache-2.0 OR MIT; retain upstream attribution and source/license inventory.

The initial reviewed resolution target is ZIP **9.0.5**, already present through Candle in Abbey's baseline lock. Do not select 9.0.0 merely because the advisory lower bound says 9: the installed 9.0.5 changelog includes subsequent symlink, permission, path and header fixes. Record the exact registry checksum and feature graph chosen by Cargo.

Before adoption, discover the resolved upstream checkout through locked Cargo metadata rather than hard-coding a machine cache path. With `ZIP_REPAIR_UPDATER_SOURCE` set to that checkout:

```sh
git -C "$ZIP_REPAIR_UPDATER_SOURCE" rev-parse HEAD
git -C "$ZIP_REPAIR_UPDATER_SOURCE" diff --exit-code HEAD -- plugins/updater
git -C "$ZIP_REPAIR_UPDATER_SOURCE" show --stat ba98bfadc4c868c9a7e1a226c83af528e1f11b89
git -C "$ZIP_REPAIR_UPDATER_SOURCE" show ba98bfadc4c868c9a7e1a226c83af528e1f11b89 -- plugins/updater/Cargo.toml .changes/updater-zip-9.md
```

Require the full commit ID, matching tracked source bytes and the exact git source in Abbey's generated lock. Cache markers such as `.cargo-ok` are not upstream tracked changes. Do not edit that cache. Preserve the prior candidate patch as evidence; resolve the current manifest rather than applying its stale lockfile hunks blindly.

### Behavior and discriminating fixtures

Source inspection of the pinned plugin establishes the relevant seam: `Update::install` calls `verify` before installation; the Windows ZIP path constructs `ZipArchive`, calls `extract(temp_dir)`, then finds a root `.exe` or `.msi`. Successful Windows installation launches an installer and exits the app. The extraction method is private: a test that reimplements extraction using a separate ZIP dependency must not be presented as actual updater proof.

Retain existing checks:

- `updater_signature.rs`: parse shipped pin; verify the real fixture and signed version/file binding. Its optional built-artifact case returns early when `ABBEY_DESKTOP_UPDATE_ARTIFACT` is unset, so record that as not exercised.
- `desktop/scripts/tests/test_assemble_update_feed.py`: version/name/signature binding and malformed/missing release inputs using isolated fixture release directories.

Add actual public-API cached verification tests: valid test-signed bytes verify; altered bytes, invalid signature/public key, mismatched announced version and missing required signed version refuse. `install(tampered_bytes)` must refuse before a platform effect. Use a test-only key/configuration and no network. This is more discriminating than copying the signature algorithm again.

Windows extraction fixtures use valid test signatures so they reach extraction. Run each in an isolated child with a private `TEMP`/`TMP` tree; ensure every potential escape destination remains inside the parent's disposable test root. Use no `.exe`/`.msi` entries in hostile fixtures, so even a regression cannot launch an installer. Assert the expected refusal **and** unchanged outside-output sentinels, no external link/file creation and a bounded child result. Cases: parent-directory and absolute/drive-relative paths; outward symlink; chained symlink escape; archive entry through a pre-existing symlink/junction; malformed/truncated ZIP. Generate dangerous metadata with Python's standard-library ZIP writer or documented raw fixture bytes, independently of the patched reader; retain generator description/hashes. Fail or explicitly mark the native case unexecuted if the host lacks required link privileges—never silently count it as passed.

Unix SUID/SGID and chmod-through-symlink fixtures belong to the ZIP-library contract verification on Unix; Windows cannot establish those POSIX permission properties. Windows ACL/reparse-point tests are separate. Do not claim a Mac signature test exercised the Windows ZIP branch.

Native positive acceptance remains an operator run in a disposable Windows VM: safe and personal editions, signed local test feed, actual NSIS/MSI ZIP update, restart/version confirmation and preserved edition state; failure paths must retain the prior installed application. No live release publication or production credentials are required for the harness. Record Windows 11 ARM64 and other supported platform rows independently.

### Commands and evidence boundaries

After the isolated manifest/lock update, from the Abbey task checkout (serialize with all other builds in that checkout):

```sh
export CARGO_TARGET_DIR="$PWD/.work/target-updater-zip9"
./tools/cargo.sh --abbey-qualify-toolchain
./tools/cargo.sh metadata --locked --format-version 1
./tools/cargo.sh tree --locked --target all -i tauri-plugin-updater
./tools/cargo.sh tree --locked --target all -i zip@9.0.5
./tools/cargo.sh test --locked -p abbey-desktop --test updater_signature --test updater_cached_verification < /dev/null
./tools/cargo.sh test --locked -p abbey-desktop --features personal-edition --test updater_signature --test updater_cached_verification < /dev/null
python3 -m unittest discover -s desktop/scripts/tests -p test_assemble_update_feed.py
ABBEY_DEP_SCAN_REQUIRE=1 ./tools/security/run-dep-scan.sh
```

Inspect locked metadata to require no ZIP 4 package and the exact updater git revision; an empty inverse tree caused by wrong target/feature selection is insufficient. Record remaining audit findings without expanding ignores. Complete the required `./check.sh` and `./desktop/check.sh` serially on the integrated cohort before calling local functional qualification complete; historical candidate passes do not cover this source. The desktop gate includes macOS acceptance and is not a universal Windows gate.

On Windows, the new PowerShell runner must locate the repository-pinned compiler through `rustup which cargo`, verify Rust 1.99.1 and the native MSVC target, create its own target/temp roots, launch tests with stdin closed and record exit status/source hashes. It must not assume the existing Unix shell wrapper is qualified on Windows. Its concrete Cargo argument sets are:

```text
test --locked -p abbey-desktop --test updater_windows_zip -- --test-threads=1
test --locked -p abbey-desktop --features personal-edition --test updater_windows_zip -- --test-threads=1
```

Run the local cached-signature suite there as well. The operator's actual installer/update acceptance is a separate explicit runner mode, never the default test mode. A Mac cross-check/build does not replace those Windows receipts.


## Report and rollback

Self-review, commit exact owned files, and report source/lock/fixture hashes, commands/exits/output, provenance, actual executed platforms and remaining obligations. The controller supplies independent review. Revert this packet to restore the prior plugin/lock; no live state or installation is changed by this task. Full root/desktop gates run serially on the later integrated repaired cohort; known remaining GTK audit failures cannot be called a pass here.
