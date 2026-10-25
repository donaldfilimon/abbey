# Abbey GTK compatibility repair

**Goal:** Fix the remaining GLib and proc-macro-error findings in Abbey without changing GTK3 APIs, package identities, or existing updater compatibility.
**Architecture:** Abbey-local attributed source patches plus independent macro/runtime regression witnesses; no dependency on Studio vendor paths.
**Specification:** Approved master milestone2 baseline security and code-first execution addendum. ABI C01/C35/C42 contribution; no whole-row qualification.
**Technology:** Rust1.99.1, GLib0.18.5, glib-macros0.18.5, gtk3-macros0.18.2, proc-macro-error2 2.0.1.

## Superseding advisory ruling: active macro target is version 3

Actual scanner and RustSec RUSTSEC-2026-0173 show that proc-macro-error2 has no
patched maintained release. The source contract below records the earlier
candidate; its active replacement is now **proc-macro-error3 = 3.1.1**, with
`default-features = false` and explicit `syn2-error`, subject to confirming the
published manifest/API. Both macro parents use the real dependency/import path
`proc_macro_error3`; do not alias it as version 1 or 2. Verify/pin the actual
version-3 and attr3 archive/source/license/feature graph, update local provenance
and locks, and repeat covering diagnostics/runtime checks and actual scans.
Preserve rejected version-2 receipts and source attribution. No new ignore or
version relabeling is permitted. This is the smallest compatibility update that
satisfies the approved security goal; cost is explicit requalification on the new
graph. Primary references: https://rustsec.org/advisories/RUSTSEC-2026-0173 and
https://docs.rs/crate/proc-macro-error3/3.1.1 .

## Global Constraints

- Only this task's Abbey checkout is writable. Adjacent ABI and WDBX are exact read-only inputs. Preserve original/untracked/ignored/unpushed work and all historical receipts.
- The approved upstream updater Git pin, ZIP9.0.5, Tokio1.54.1 and ctutils0.4.4 remain. No package-version relabeling, advisory suppression, GTK4 migration, new sibling vendor dependency or credential/live action.
- Preserve all source/license/metadata attribution; distinguish pristine archive hashes from patched file hashes. Registry sources and cached Git checkouts are read-only.
- Use an independent excluded vendor workspace with its own patches/lock; default fixtures are macro-only, optional runtime features explicit. Existing consumer protocol/generated TypeScript unchanged.
- Use Abbey tools/cargo.sh for every Cargo invocation, separate task-owned targets, stdinEOF, ABI_WDBX_PERSIST unset and ABI_WDBX_PATH=:memory:. CARGO_INCREMENTAL=0 and DEV/TEST_DEBUG=0 bound caches; record command/selected-env/exit/source receipts privately with concise portable published summaries.
- Parallel independent tasks are explicitly authorized. No shared writable target, store, port or source; serialize heavy gates/benchmarks through controller. Check available disk and report below10GiB.
- Native Linux/GTK and Windows qualification must remain explicitly unexecuted when unavailable; detection/audit omission is not a source/runtime proof.

## Task 1: Apply and verify Abbey-local GTK source compatibility patches

Ownership is the **Abbey owner** listed below only, including its consumer/vendor manifests and locks, local vendor trees, source provenance verifier/tests and necessary gate wiring. Studio paths below are read-only context for the separate consumer task. Do not edit adjacent WDBX/ABI or their instructions. Use the already prepared exact source/diff/fixture contract below; no new broad planning.

Local prerequisites were probed: pkg-config reports GLib and GObject2.90.1 on this Mac, but no GTK3. Run actual positive GLib/optimized iterator fixtures here in dev/release when linkage succeeds. Macro-only positive/negative compilation runs locally. Deliver the GTK-native/Linux runner with explicit unexecuted status; do not install system SDKs silently or equate Mac GLib with Linux GTK.

# GTK 3 compatibility repair: Abbey and WDBX Studio

Read-only execution draft, 2026-10-25. No source/Git edits or tests were performed. The only write is this draft. Inspected Abbey's frozen baseline and preserved security candidate, and WDBX task005 `81ae29a7c472d42765fd1f09f75c1288b0252d30`, including Studio's current vendor copy. Integrate after each consumer's ZIP repair and current compatible-lock changes; do not transplant an older whole lockfile. All paths below are relative to the named product repository.

### Decision and exact scope

Use the same reviewed compatibility patch in **two independently owned consumer tasks**, with local source copies in each repository. Abbey owns `vendor/`; Studio owns `specimen-studio/native/vendor/`. Neither depends on the other's vendor directory. Keep GLib 0.18.5, glib-macros 0.18.5 and gtk3-macros 0.18.2 package identities. Backport the authoritative GLib fix, and migrate **both** macro crates to the maintained proc-macro-error2 API. This is a compatibility backport, not a GTK 4 migration or a claim that GTK 3 itself is newly maintained.

Abbey's preserved `docs/verification/2026-10-22-completion-recovery/security-candidate.patch` was the updater/dependency candidate; its dependency scan still reported GLib and proc-macro-error. Its recorded root/desktop passes do not validate this new GTK patch. Preserve all original candidate files and receipts. Studio already owns the GLib backport: comparing its entire original crate archive to the vendor copy found only `src/variant_iter.rs` changed. Reuse and retain that work.

### Original source, checksums and attribution

Read locally from Cargo's `registry/cache/index.crates.io-1949cf8c6b5b557f/` and corresponding `registry/src/`. Each archive below was hashed during this audit. Source URL is `https://static.crates.io/crates/NAME/NAME-VERSION.crate`; verify again before copying. Use complete archive contents, including `LICENSE`, README, `Cargo.toml.orig`, `.cargo_vcs_info.json`, tests and attribution. Do not copy generated registry bookkeeping or claim an original checksum describes patched bytes.

| Crate | Original archive SHA-256 | Declared license | Archive VCS identity |
| --- | --- | --- | --- |
| glib 0.18.5 | `233daaf6e83ae6a12a52055f568f9d7cf4671dabb78ff9560ab6da230ce00ee5` | MIT | gtk-rs-core `42b9caf98e03ded086362d9653ca58fe94dc8658`, `glib` |
| glib-macros 0.18.5 | `0bb0228f477c0900c880fd78c8759b95c7636dbd7842707f49e132378aa2acdc` | MIT | same commit, `glib-macros` |
| gtk3-macros 0.18.2 | `52ff3c5b21f14f0736fed6dcfc0bfb4225ebf5725f3c0209edeec181e4d73e9d` | MIT | gtk3-rs `00133512bfc8bbb8e9be59b20e93406c6a3eeb21`, `gtk3-macros`; metadata explicitly says `dirty: true` |
| proc-macro-error2 2.0.1 | `11ec05c52be0a07b08061f7dd003e7d7092e0472bc731b4af7bb1ef876109802` | MIT OR Apache-2.0 | GnomedDev/proc-macro-error-2 `9b129234ed8f50d3ff8da09041dcbd980b63fc8c`, root |
| proc-macro-error-attr2 2.0.0 | `96de42df36bb9bba5542fe9f1a054b8cc87e172759a1868aa05c1f3acc89dfc5` | MIT OR Apache-2.0 | same upstream repository, `4302a1d749def2b2929b1292bf418bd58df04486`, `proc-macro-error-attr` |

The three gtk-rs `LICENSE` files share SHA-256 `8cf56d10131ce201cf69ab74b111d3ebac1acca3833d7efb39ae357224b70edb`. Both replacement crates' `LICENSE-MIT` hash is `544b3aed1fd723d0cadea567affdcfe0431e43e18d997a718f9d67256b814fde`. Their Apache license hashes respectively are `4665f973ccb9393807a7fb1264add6b3513d19abc6b357e4cb52c6fe59cc6a3b` and `6fd0f3522047150ca7c1939f02bc4a15662a4741a89bc03ae784eefa18caa299`. Keep these exact supplied files; differing upstream license bytes are not an instruction to normalize them. This is provenance/license inventory, not complete distribution-license clearance.

The proc-macro-error2 crates can remain registry dependencies with locked checksums; only the three gtk-rs crates need local modification. Record original file hashes, the narrowly allowed patch, and resulting source hashes in a per-consumer vendor manifest. A verifier must reject drift in licenses/metadata, package-version substitutions, unexpected source changes, or a macro dependency silently returning to proc-macro-error 1.x.

### Minimal production diff

1. `glib-0.18.5/src/variant_iter.rs`, `VariantStrIter::impl_get`: change `let p` to `let mut p` and `&p` to `&mut p` for the C out-parameter. This is exactly [upstream commit b5a4071e439bef2b5eea76c3aa25e5ae84839e34](https://github.com/gtk-rs/gtk-rs-core/commit/b5a4071e439bef2b5eea76c3aa25e5ae84839e34), merged through [gtk-rs-core PR 1343](https://github.com/gtk-rs/gtk-rs-core/pull/1343). The upstream rationale and diff were checked directly. Studio already contains these two changes; Abbey needs its own copy.
2. Both macro crates' active `Cargo.toml`: replace `[dependencies.proc-macro-error] version = "1.0"` with `[dependencies.proc-macro-error2] version = "2.0.1"`. Mirror this dependency change in `Cargo.toml.orig`, documenting it as a local modification. Keep package names/versions, syn 2, proc-macro2, quote, proc-macro-crate and features otherwise unchanged. The resolved lock must select proc-macro-error2 2.0.1 and its exact attr2 2.0.0 dependency for this reviewed packet.
3. Replace the crate path `proc_macro_error::` with `proc_macro_error2::` in the following exact source files. Keep the imported attribute's name `proc_macro_error` and existing `#[proc_macro_error]` uses; do not change macro behavior:
   - glib-macros: `src/lib.rs` (four paths), `boxed_derive.rs`, `closure.rs`, `derived_properties_attribute.rs`, `enum_derive.rs`, `error_domain_derive.rs`, `flags_attribute.rs`, `object_interface_attribute.rs`, `object_subclass_attribute.rs`, `shared_boxed_derive.rs`, `variant_derive.rs` (one path each).
   - gtk3-macros: `src/lib.rs`, `src/composite_template_derive.rs` (one each).

**Do not alias the new dependency as `proc-macro-error`.** The inspected attr2 source generates `::proc_macro_error2::entry_point(...)` at `src/lib.rs:96`. A manifest-only alias leaves that path unresolved. Both dependency paths matter: GLib reaches glib-macros, while GTK reaches gtk3-macros; changing only one leaves proc-macro-error 1.0.4 in the graph.

No other source migration is assumed proven until the macro fixtures compile/run. An additional required compatibility change must be separately explained and tested, not hidden in a vendor refresh. Retain the original gtk3-macros description and metadata, including its unmaintained description and dirty VCS marker; the crate archive is the exact source identity.

### Owned files and workspace boundaries

**Abbey owner:** root `Cargo.toml`, root `Cargo.lock`; new `vendor/{glib-0.18.5,glib-macros-0.18.5,gtk3-macros-0.18.2}/`; `vendor/README.md`, `vendor/source-manifest.json`, `vendor/Cargo.toml`, `vendor/Cargo.lock`, and `vendor/compat-tests/`; a bounded `tools/security/check_gtk_vendor.py` provenance/resolution verifier with focused tests. Add an explicit vendor verification/test invocation to the appropriate gate/CI only after the fixtures exist. `desktop/` is a member of the root workspace: do not create `desktop/Cargo.toml` or `desktop/Cargo.lock`.

**WDBX Studio owner:** `specimen-studio/Cargo.toml`, `specimen-studio/Cargo.lock`; the same three crate copies under `specimen-studio/native/vendor/` (retain the existing GLib tree); its README/source manifest and independent vendor workspace/lock/compat-tests; bounded verifier under `specimen-studio/scripts/` with focused tests. Keep the current `[profile.dev.package."glib@0.18.5"] opt-level = 3`. No WDBX root manifest/lock, fuzz lock, ABI source, episode format, model package, specimen kernel behavior, or ZIP import/export changes.

Each **consumer workspace root** needs three `[patch.crates-io]` entries pointing to its own local copies. Preserve the updater patch from the ZIP task. Exclude the vendor workspace/packages from automatic consumer membership. The vendor directory is a small **independent workspace** containing the three crates and compat-tests, with its own three patches, lockfile and optimized-GLib dev profile. Root patches/profiles are not inherited when testing an excluded vendor workspace directly. Check metadata from both roots: GLib 0.18.5 and both 0.18 macro crates must resolve to the intended local paths, with no registry 1.x error crate; any unrelated GLib 0.21 graph in Studio remains distinct and untouched.

Regenerate locks with Cargo after adding patches; inspect the resolved delta and serialize integration with ZIP/compatible-lock owners. Preserve product package versions and third-party notices. Do not hand-edit lock rows, falsify a version to evade an advisory, or introduce cross-repository vendor paths. Generated desktop TypeScript/IPC bindings are untouched; run their existing drift check rather than regenerating unrelated output.

### Independent fixtures and meaningful evidence

Add independently authored cases in each local `vendor/compat-tests/` and retain upstream tests. The harness has a lightweight macro-only mode and explicit `glib-runtime`/`gtk-runtime` feature modes; avoid accidentally building GTK through an unconditional test dependency on macOS. Declare `gtk` as an optional dependency even in macro-only mode: gtk3-macros calls `proc_macro_crate::crate_name("gtk").expect(...)` before parsing the template, so the manifest must name it, although the feature remains disabled and the negative expansion stops before generated GTK code. The fixture subprocess manifest must preserve that declaration. Its patches must route runtime GLib/GTK reexports back to the same local macro crates.

- **Macro-only negative fixtures:** invoke `glib_macros::Enum` on a struct and on an enum lacking `#[enum_type(name = ...)]`; invoke `gtk3_macros::CompositeTemplate` without `#[template(...)]`, plus a malformed template-child attribute. Require the expected macro diagnostic and nonzero compilation, with no panic/unresolved `proc_macro_error2`/missing-crate error accepted as success. These exercise abort/error emission without needing GTK libraries. Pin compiler identity; review diagnostic snapshot changes rather than auto-blessing them.
- **Positive GLib fixture:** derive an independently named GLib enum with a configured name and explicit values, convert through `Value`, and assert exact roundtrip/type behavior. Also exercise one valid `glib::clone!` closure so the migrated closure path runs. This requires native GLib; a metadata or macro-crate-only check is not this execution.
- **Positive GTK fixture:** a minimal real GLib object subclass implementing `gtk::subclass::widget::CompositeTemplate`, with an inline template and a declared `TemplateChild`, compiling through the patched public `gtk::CompositeTemplate` derive. Native Linux runs an instance/bind-child assertion under a declared virtual display if initialization requires one. A compile-only fixture may be the first step, but report its narrower result. Do not invent a fake GTK API to make expansion compile.
- **Optimized GLib regression:** retain Studio's existing Linux-only `native/specimen-core/tests/conformance.rs::optimized_glib_variant_iteration_backport`; it checks forward/reverse/nth/nth_back/last. Add the corresponding independent fixture for Abbey's vendor workspace, with empty, single-item, mixed front/back and Unicode string arrays. Run dev with GLib opt-level 3 and release. Include upstream `variant_iter` tests. Do not count the `#[cfg(target_os = "linux")]` test as executed on macOS.
- Verify fixture graph identity and source hashes before/after execution, and assert that invalid examples fail for their intended macro reason. Source inspection alone cannot establish that all expanded macros still behave correctly.

### Commands and platform qualification

Resolve tools from the pinned stable 1.99.1 toolchain. In Abbey use `./tools/cargo.sh` for Cargo; Studio uses Cargo directly. Set a separate writable target directory and stdin EOF per consumer. Let `VENDOR` mean `vendor` in Abbey and `native/vendor` when working from `specimen-studio/`.

Local host checks, after lock generation:

```sh
cargo metadata --locked --format-version 1
cargo metadata --locked --manifest-path "$VENDOR/Cargo.toml" --format-version 1
cargo check --locked --manifest-path "$VENDOR/Cargo.toml" -p glib-macros -p gtk3-macros --lib
cargo test --locked --manifest-path "$VENDOR/Cargo.toml" -p gtk-compat-tests --no-default-features --test diagnostics < /dev/null
```

The macro check compiles the attribute-generated entry-point references. The diagnostic fixture actually invokes macro expansions. Neither establishes Linux GTK runtime behavior. A Mac GLib-positive run is additional named-host evidence only if its native GLib prerequisites are present and its test actually executes.

Native Linux, with GLib/GTK 3 development libraries and a recorded display setup:

```sh
cargo test --locked --manifest-path "$VENDOR/Cargo.toml" -p gtk-compat-tests --features glib-runtime,gtk-runtime < /dev/null
cargo test --locked --manifest-path "$VENDOR/Cargo.toml" -p gtk-compat-tests --features glib-runtime,gtk-runtime --release < /dev/null
cargo test --locked --manifest-path "$VENDOR/Cargo.toml" -p glib --lib variant_iter < /dev/null
cargo test --locked --manifest-path "$VENDOR/Cargo.toml" -p glib --lib variant_iter --release < /dev/null
```

Studio also runs its existing exact optimized regression in both profiles: `cargo test --locked -p specimen-core --test conformance optimized_glib_variant_iteration_backport -- --exact`, then the same with `--release`. Assert the runner reports **one executed test**, not zero selected. Full consumer validation remains Abbey `./check.sh` plus `./desktop/check.sh`; Studio `bun run check`, `bun run test:native`, and `bun run check:native`. WDBX's root gate excludes Studio. Native Linux desktop gates and native Windows consumer build/install tests are separately recorded; this audit executes neither. GTK usually is not selected by the Windows application graph, so Windows success cannot qualify the GLib C path.

### Strict dependency reporting and remaining separate owners

Do not add any advisory ignore or lower warning severity. Capture fresh raw `cargo audit --deny warnings` output and database identity; retain a failing exit code whenever findings remain. Abbey's required integration command remains `ABBEY_DEP_SCAN_REQUIRE=1 ./tools/security/run-dep-scan.sh`; its preexisting bincode/paste exceptions must be disclosed, not expanded or described as a clean exception-free scan. Preserve the missing-scanner failure. Run the vendor provenance/resolution check independently: path dependencies can disappear from registry advisory matching, so disappearance of the GLib row alone does not prove the backport.

Studio's recorded task005 scan still has ZIP (separate repair), proc-macro-error, paste, five unic warnings and two lru unsoundness warnings. GTK success must not be reported as a whole-graph pass.

- **unic:** current route is `tauri-utils 2.9.3 -> urlpattern 0.3.0 -> unic-ucd-ident 0.9.0` and the other unic crates. Cached authoritative manifests show tauri-utils 2.10.2 uses urlpattern 0.6, which removes that unic dependency. Current Studio Tauri callers request compatible `2.9.3`/`2.8` ranges, so a separate lock-only candidate update of tauri-utils to 2.10.2 is plausible; it is **not resolved or tested here**. Its Rust 1.90 floor fits the product's 1.99.1. Give it a separate lock owner and URL-pattern/permissions/codegen/desktop regression scope. If resolution or behavior requires source changes, report them; do not force a cross-version urlpattern patch.
- **lru:** current Studio route is `specimen-tui -> ratatui 0.29.0 -> lru ^0.12`. The inspected advisory database requires at least 0.18.2 to cover both current lru issues; that cannot resolve within ^0.12. Cached ratatui 0.30.2 uses ratatui-core 0.1.2 with lru ^0.18, a maintained candidate route with Rust 1.88 floor. This requires a **separate source/manifest-owned TUI migration**, beginning with `specimen-studio/native/specimen-tui/Cargo.toml` and its TUI source/tests. It is not a GTK patch or an asserted drop-in upgrade. Preserve layout, Unicode width, input, read-only-lock fallback and rendering fixtures; resolve a fixed lru version in its own reviewed lock delta.

Finish this packet with original/post-patch manifests and hashes, exact dependency-source graph, license inventory, positive and negative macro receipts, and named-host runtime outcomes. No receipt is fabricated for Linux/Windows, no original evidence is rewritten, and no generated TypeScript is changed merely to make the dependency work appear complete.

## Acceptance and rollback

Actual root/vendor metadata selects all three local patched sources and no proc-macro-error1.0.4. Source/provenance/license negative controls and macro diagnostics pass; execute available GLib runtime/optimized iterator fixtures and report unavailable GTK/platform cases. Run Abbey's existing mandatory security scanner unchanged and focused desktop/runtime checks; authoritative root/desktop gates run serially after this packet's final source and other prerequisite repairs are ready. A green registry audit alone does not prove vendored GLib safe. Self-review, commit only owned files, provide command/exit/output/source/lock hashes and independent review inputs. Revert task commits to restore prior resolution; no runtime/store migration occurs.
