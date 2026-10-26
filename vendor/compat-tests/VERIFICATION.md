# Task009 verification — 2026-10-25

At initial producer commit `e49ccbd`, both authoritative gates passed on its frozen sources and proc3 graph: `./check.sh` exit0 and `./desktop/check.sh` exit0. Host: aarch64-apple-darwin; rustc1.99.1 (`58cf8f9c35b355199727627d07ae34a4e2846d67`); Cargo1.99.1; GLib/GObject2.90.1. GTK3 SDK absent.

| Receipt | Exit | Observed result | Log SHA256 |
| --- | --- | --- | --- |
| `root-gate-final` | 0 | Full root gate; all cohorts/docs/guards/installer pass | `ddaffcc0f3f4a2081bf5db899e8f1c9eeadeabb92c67b40be40f741d55cab251` |
| `desktop-gate-final` | 0 | Full desktop gate;178 frontend tests,6 live-daemon tests,linked binary | `0d03249025e1a4eb9ea479d1a9f298566298e4bbb355b00c5ebd7825b396ae35` |
| `final-verifier-before-frozen-gates` | 0 | Both actual metadata roots and independently pinned sources/licenses/locks/features | `be2b31b3b35d4b984b850c2998065e67a032e8102ca7707fac12c3567ddcb9ae` |
| `system-python-controls` | 0 | 18 provenance/resolution controls on system Python3.9 | `a90b6e6f8d8bc5b32625a3c5da1cb9e37483814b0e86f2e3dffbeabef732db7e` |
| `proc3-diagnostics-final` | 0 | Four intended compile failures in one passing macro harness | `f45fc2b246733becb763d2d01e4539607a6d3f514481cb82fc4e3caf056e759c` |
| `proc3-glib-dev` | 0 | 2 GLib runtime witnesses; GLib opt-level3 | `c17c4a463e760fe95b8902fb51f5f419cdf837274a21193bb2c4a48fc80c9675` |
| `proc3-glib-release` | 0 | 2 GLib runtime witnesses | `59cc900982e9cbac7b24d4edd80020b2827ba89121a164fd8e84e4e174959cd2` |
| `proc3-upstream-dev` | 0 | 11 upstream variant_iter tests | `1e5380304631244894ed1697bba04a80b777f2081f20d78c138f19fe49a257c7` |
| `proc3-upstream-release` | 0 | 11 upstream variant_iter tests | `7f203f7d99d1afb8eb50956999a977af694430e53f33f64808e29f1569cfce0e` |
| `proc3-macros-check` | 0 | Both macro libraries compile | `ba064f3146534c78d6f71fa0b46e504f8d9c59315c8fca1d12554f6d7c38befa` |
| `proc3-codegen` | 0 | 5 generated files unchanged | `86d178483fdaa364016c64157fd0b0356166668e080e1d485e0db53fa63307db` |
| `proc3-dep-scan` | 0 | Mandatory scanner passes with existing2 exceptions | `ce8f7d7601f06486c4957c6a1960fdf86056240a00b8fbd82efcfb590e974a0a` |
| `proc3-audit-raw` | 1 | Only existing bincode/paste warnings remain | `59c8b85750d248e63c78343324e11f6e3eacc5c4ae08a2d67eadd709c4317a82` |
| `proc3-vendor-audit` | 1 | Retained upstream test tooling basic-toml warning | `ed42fdadfa6599965b821430d7d274b1ea952dcac48a53c21445a6a836f6f66c` |
| `linux-runner-host-guard` | 2 | Linux GTK runner refuses unsupported Mac | `8406c4b606cbf223e092214a8c63f7b5ae054b9c169ab169b9a9f7a840b515fc` |

The full gates share source inventory SHA256 `e7561210a0ff511677c1e947ec00ebdfad8774f19cce0f1c91c8ba51c268cdc5` before/after each command. The source remained frozen through both runs. The vendor/runtime sources, provenance verifier and locks remain unchanged. The later test-setup change is qualified by the targeted fix-round checks below. Controller-authorized focused builds in other tasks used isolated targets during the latter gate stages. These are functional checks with concurrent load, with no controlled-performance claim.

Every Cargo command uses `tools/cargo.sh`. Exact commands, selected environment, stdinEOF, exits and unique source inventories are private under ignored `.work/gtk/receipts/`; this summary publishes portable receipt names and hashes. Environment: ABI_WDBX_PERSIST unset, ABI_WDBX_PATH=:memory:, CARGO_INCREMENTAL0, DEV/TEST debug info0. Separate consumer/vendor targets are under `.work/gtk/`; the consumer target is an independent APFS clone of the updater cache, not shared writable state or qualification by itself. Existing supported ABBEY_ZIP_OPENSSL/PYTHON overrides are resolved before the wrapper.

Regression evidence: the independently authored optimized string-array test aborted against pristine GLib0.18.5 (exit101/SIGABRT, invalid C string pointer), then passed the exact upstream mutable out-pointer backport. Positive enum/Value/exact discriminants, clone!, empty/single/Unicode/mixed iterator cases pass in dev/release. Four negative macro fixtures require their intended diagnostics and reject panic, unresolved/missing crate and additional compiler errors. Compiler release1.99.1 is checked. Symlink-boundary tests first failed before two narrow guards;18 controls now pass. The verifier reuses the existing structural TOML parser fallback; actual system Python3.9 RED/GREEN and controls pass.

Historical failures remain preserved: proc-macro-error2 2.0.1 candidate strict/mandatory scans failed RUSTSEC-2026-0173; the controller amended both macro parents to real proc-macro-error3=3.1.1, defaults disabled, only syn2-error. Initial full root gate exit101 selected Apple OpenSSL during updater fixture generation; initial desktop gate exit1 lacked frozen frontend dependencies. Supported runtime-resolved overrides and frozen bun installation repaired setup, then both complete gates were rerun successfully on the final sources. No original failure receipt was overwritten.

Advisory database: `21d909e4c8bd4e14c7aea58bf980c881a0053ab1`; cargo-audit0.22.2. The unchanged mandatory scanner passes under exactly its existing RUSTSEC-2025-0141 (bincode1.3.3) and RUSTSEC-2024-0436 (paste1.0.15) exceptions. Raw consumer strict audit retains exit1 on those two warnings. No new exception, lowered severity or missing-scanner bypass was added.

Optional vendor strict audit retains exit1 on RUSTSEC-2026-0374 basic-toml0.1.10 through upstream dev-only trybuild2. The advisory has `patched = []` and no unaffected range. GLib dev requires trybuild2^1; glib-macros dev requires^1.1. Selected trybuild2 1.3.0 requires basic-toml^0.1. The inspected registry index has no newer route and all>=1.1 releases retain that dependency;1.0.0 used toml but violates the macro-parent constraint. No compatible lock-only repair is demonstrated; upstream test manifests/sources are preserved.

Source/license/archive/VCS inventories and independent review pins are in `source-manifest.json`, `README.md` and `tools/security/check_gtk_vendor.py`. Direct complete-archive comparison found exactly1 changed GLib file/two lines,13 changed glib-macros files,4 changed gtk3-macros files. All unrelated root lock package records remain structurally identical, including updater Git pin, ZIP9.0.5, Tokio1.54.1 and ctutils0.4.4. Package versions and all supplied license/metadata bytes remain. No source-size exclusion or new ignore was needed.

Native Linux GTK3 CompositeTemplate/TemplateChild compilation/runtime and native Linux/Windows application qualification remain **unexecuted**. Linux/Windows Rust cross targets were absent, so the existing soft guard skipped them. The real GTK fixture/xvfb runner is delivered and explicitly rejects this Mac. Optional macOS GUI/package acceptance flags were not enabled. Mac GLib and the local root/desktop gates do not qualify those cases. Upstream warnings remain unmodified.

Stable source/lock digests:

- `Cargo.toml`: `20723577214c168b4f05e9364d097fc447843d3e4699162bd3504d15d62de2a8`
- `Cargo.lock`: `39a91000c56653a900ee264bbe5cfe9512db72b0b585a89de3ca32635df5dcb4`
- `check.sh`: `54be875df405cb4c2a8ef7fc65da032bad4505b66983c6c644215e08b7b79d8c`
- `tools/security/check_gtk_vendor.py`: `7d0233a256ca92757e7806ff17a0f0c3a5743bd1f2466f0adc4db05bd8b5e62c`
- `tools/security/tests/test_gtk_vendor.py`: `8def570896ee1d990136a4218e3ecddc656701ae6da4e8582c73492c7a8774dd`
- `vendor/Cargo.toml`: `93c9a9bffea983c50e13d59e8349edcdcae008f9c75517f548c4c81eee66bef0`
- `vendor/Cargo.lock`: `ffa28931ad41c95b73d658ea0343b8be6fc179dfc5ea71b726e26c3ca97ac8a9`
- `vendor/source-manifest.json`: `e5a117e3f2d29f3a5199dee9e7b841b131c8cf391e81362815e849b70132bffb`

Packaging whitespace is deliberately split: owned code/fixtures/manifests pass
staged whitespace checks. The unscoped staged check reports four inherited
archive entries: trailing blank lines in the three supplied LICENSE files and
one trailing space in an original glib-macros documentation line. Complete
archive comparison proves these bytes unchanged; no license normalization,
source cleanup or whitespace bypass was added.

## Fix round1 — fresh-checkout controls scratch parent

Review I1 found that mutation controls required `.work/gtk` before the gate's
first vendor build. Shared `setUpModule()` now creates that parent with
`parents=True, exist_ok=True` before either test class runs. No vendor/runtime,
checker, gate, lock, license or original cache bytes changed.

RED used a private tracked-source copy with an absent scratch parent;18 controls
ran with9 FileNotFoundError errors. The actual task scratch/cache directories
were never renamed, deleted or used as the absent-parent fixture. GREEN ran18
controls under current Python and system Python3.9, plus the concrete gate section
in independent absent-parent source copies for both interpreters: full verifier,
18 controls, macro diagnostic harness, full verifier. Metadata left the scratch
parent absent; shared test setup created it before Cargo diagnostics. All focused
commands passed, with4 intended diagnostic cases per harness run. Separate private
targets and jobs2 were used; these were functional concurrent-load checks.

The earlier root/desktop success receipts retain their exact earlier source
inventory. Those whole gates and installer suites were not repeated for this
test-only fix. Existing warning, advisory and platform qualification limits above
remain; no warning suppression or warning-free claim was added.

| Fix-round receipt | Exit | Log SHA256 |
| --- | --- | --- |
| `fixround1-fresh-parent-red` | 1 | `560893cb2642e10b3c15f433ae0010ad8cb4540265e252b8bc589f6dd5e04372` |
| `fixround1-controls-current` | 0 | `a200be47323d80c8828f3e945476c2f815ca04eb857321c860460b1796e9e7b4` |
| `fixround1-controls-system` | 0 | `9f4197f4a52b0fa5754ea75c6a0a66c3e69f54282865a1ae16a4b89702a1931e` |
| `fixround1-fresh-gate-current` | 0 | `6d0832540f3fbb64f4cf0d664f3234957e680d3b49c5d2cc1bb9256b2869f259` |
| `fixround1-fresh-gate-system` | 0 | `e0e5e0cdbb50673383cb651de3da619134f85d1ca15110e51840ce6863256155` |

Final test setup SHA256: `7d3c511b2d76258a42543832b6cef70ccaefe0e4da7eba78313e6202f55263d6`. The earlier digest table describes the initial producer snapshot.

## 2026-10-25 maintained DEV harness qualification

The isolated maintenance packet replaces trybuild2/basic-toml with pinned
trybuild1.0.122 in the excluded vendor DEV graph. Root lock, production macros,
iterator implementation, runtime and Studio sources remain unchanged. Registry
archive/license/VCS facts and source inventories are independently pinned.

Rust1.99.1 through tools/cargo.sh, single build job, dev/test debug info0,
incremental0, offline execution, stdinEOF and a private target: all four final
receipts bind source3fe5fb9 before/after.

| Check | Exit | Raw log SHA256 |
| --- | --- | --- |
| clone-final | 0 | `8676e7b77d2542604a2d9b8efcb7c878f86a39f788c5d2625be681ac8a61fc24` |
| subclass-final | 0 | `d4490757a8fb3b6a9b4a4ed2593de4242f9398bc82fb69c2b0fce4ef789d1c1a` |
| diagnostics-final | 0 | `815ae8a4eff7c06c1b2fb97ee45a07b0fd8798b82e5431d9e1f9201a0b2cf48a` |
| glib-runtime-final | 0 | `a5be2a44378f7664f1427d2450f3186423d8f9a1d66c8188d25d0f431dfa970a` |

Clone: four Rust tests, including all29compile-fail and4compile-pass cases.
Subclass: one Rust test executes bothpass and all4compile-fail cases. Original
macro diagnostics: one test with4negativecontrols. GLib runtime:2witnesses.
No skips or zero-test acceptance in these selected suites. Existing upstream
compiler warnings remain visible. Functional observations under concurrent
machine activity provide no controlled-performance or native Linux/Windows claim.

Clone full expectations, source and carets remain checked after removing only
renderer location lines/numeric gutterlabels from the retained upstream string
comparison. Maintained trybuild additionally checks complete current snapshots.
A normalization regression rejects altered message, source and caret.
Four subclass expectation changes are explicit renderer-only transformations
(qualified internal type names, gutteralignment, long-lineellipsis); primary
E0277 diagnostics and all originalsourcecases remain unchanged. The four supplied
original snapshots are retained byte-exact in upstream-ui/original-subclass-snapshots
and checked against immutable pristinearchive hashes. No overwrite mode was used.

Final independent review found one Python test-entrypoint issue: its main guard
preceded the new harness tests. Direct selection reproduced missingclass RED;
moving the guard to the end gives5/5direct harnesscontrols,25/25direct full tests
and25/25discovery tests. This finalfix changes no Rust source or lockedgraph.
The standalone25test suite and normal discovery now exercise identical controls.

Vendor strict audit --deny warnings --no-fetch exits0:142dependencies, zero
advisories, zero warnings and empty ignorelist. Generated trybuild subproject
locks also audit with zero findings/warnings/ignores; their exact hashes and
reports are retained privately. Source/lock/both workspace metadata validation
passes after execution. The earlier basic-toml failure remains in the historical
section above and is superseded only for this reviewed DEV cohort.

No Abbey root gate or overall platform/product/production qualification is
claimed by this packet; the modernization owner retains integrated cohort gates.
