# Abbey dependency-security qualification — 2026-10-26

Status: local qualification evidence assembled; broader remediation remains
`in_progress`. Existing pins are preserved. No dependency or
production source change is justified by the current findings. This report
supersedes historical candidate status only for checks explicitly rerun below.
Original failed candidates and receipts remain intact.

## Identity and custody

Canonical `main` started at `d34e25842bc5ef6299f3b94ffdd75f85d2cd7fb4`.
The user explicitly released the Ralph/TUI writer for this qualification window.
Existing dirty CLAUDE/Ralph/TUI/PTY work remains owner-held and untouched.
Subsequent process inspection found an owner prediction stress loop with 20
CPU-load children still active, then a follow-up shutdown test in this checkout.
`src/tui/tests/prediction_producer.rs` also changed during the qualification
window. The first two root attempts were stopped by this task (exit 143),
retained as interrupted evidence, and cannot establish frozen gate qualification.
User instructed waiting for the owner to finish; no owner process was interrupted.
Read-only adjacent ABI started at `63d2bc0a393c3c3837eadab0d18bc2fc1fff5c78`;
WDBX at `612d3570f9ab0d842519aa4dbc4cb977aa33c9e0`, with unrelated dirty spatial3d
and execution/planning work. CI pins are ABI
`7b9519b6cfb25cc9bc9db2982e0a901318ba039b` and WDBX
`5182bc3a60300e1dd152611025e774c4b906638b`; local sibling results do not qualify
those archived revisions. Sibling receipts capture HEAD/status/tracked diffs,
not complete untracked or ignored sibling byte inventories. Compiler qualification uses `./tools/cargo.sh` and
Rust 1.99.1, macOS ARM64.

Exact command arrays, times, exits, output hashes, before/after Git identities,
diff digests and source/lock inventories are retained under ignored
`.work/security-qualification-20261026/`. These private receipts include actual
machine paths; this report uses relative project paths. No credentials or live
application state are used. Original attempts are retained under distinct names.

## Findings and retained fixes

Initial advisory database revision: `115e418ee9a2ff542fee41db641128fa84483383`.
Final refreshed revision: `a78282a1414d6c28cc228c4eae3570451affb0dc`; the final
consumer policy/vendor strict scans still pass and the no-ignore consumer scan
still fails on exactly the two existing maintenance exceptions.

| Finding | Current reviewed cohort | Disposition |
| --- | --- | --- |
| Tokio RUSTSEC-2026-0384 Interval race | 1.54.1 | Patched range starts at 1.54.1; preserve lock |
| Tokio RUSTSEC-2026-0386 AsyncFd I/O safety | 1.54.1 | Within patched >=1.53.3 range |
| ZIP RUSTSEC-2026-0367/0368/0369 | 9.0.5 | Patched range >=9.0.0; preserve exact package |
| Tauri updater Windows ZIP4 route | updater 2.14.0 Git `ba98bfadc4c868c9a7e1a226c83af528e1f11b89` | Locked metadata resolves ZIP9; cached updater Git HEAD matches and tracked updater directory is clean |
| GLib RUSTSEC-2024-0429 VariantStrIter | local 0.18.5 | Keep GTK3 package identity and exact upstream two-line mutable out-pointer backport; registry scan alone cannot validate this local source |
| proc-macro-error RUSTSEC-2024-0370; successor2 RUSTSEC-2026-0173 | proc-macro-error3/attr3 3.1.1 | Both old families absent from verified root/vendor graphs; both macro parents use real successor3 dependency with only syn2-error |
| DEV trybuild2/basic-toml RUSTSEC-2026-0374 | trybuild 1.0.122 | Current vendor graph strict audit passes; historical failed harness receipts remain |

Independent archive comparison confirms the original archive inventories and
GLib iterator source equals the pristine source plus exactly the two mutability
changes. Complete vendor changes also include the previously reviewed DEV
harness migration; this is not a claim that every GLib archive change is limited
to the iterator. Licenses, source inventories and independent hash pins are
checked by `tools/security/check_gtk_vendor.py` and its mutation controls.

## Fresh checks

The portable [checks summary](2026-10-26-dependency-security-qualification/checks-summary.json)
records exact argument arrays (with declared publication substitutions), selected
build settings, UTC times, exits, source/lock identities and raw receipt/output
hashes. Original commands and logs remain under the relative private references.

| Receipt | Exit | Observed result |
| --- | --- | --- |
| `toolchain` | 0 | Wrapper confirms Rust 1.99.1 / aarch64-apple-darwin |
| `provenance` | 0 | Full root/vendor metadata, local source identities, locks, licenses and macro-parent/Syn2 edges |
| `controls` | 0 | 25 verifier mutation/regression tests |
| `audit` | 0 | Mandatory repository-policy consumer scan, 814 dependencies, existing exceptions unchanged |
| `consumer-strict-audit` | 1 | No ignores: exactly bincode RUSTSEC-2025-0141 and paste RUSTSEC-2024-0436 denied |
| `vendor-audit` | 0 | Strict vendor audit without ignores |
| `updater-source`, `inverse-trees`, `sibling-pins-print` | 0 | Locked metadata, Tokio inverse tree, CI pin identities; 43 relevant inverse edges also extracted into private inventory |
| `diagnostics` | 0 | Four intended negative macro cases; unrelated tracked diff changed during this initial receipt |
| `diagnostics-final` | 0 | Same fixture rerun; source and lock inventories stable |
| `clone` | 0 | Four Rust tests including 29 compile-fail and four compile-pass upstream cases |
| `subclass` | 101 | Invocation omitted required compiletests feature; no subclass cases executed |
| `subclass-corrected` | 0 | Corrected feature selection; two compile-pass and four compile-fail upstream cases; vendor/locks unchanged, unrelated prediction test changed |
| `runtime-dev`, `runtime-release` | 0 | Two GLib runtime witnesses per profile, including optimized string iteration |
| `iterator-dev`, `iterator-release` | 0 | 11 selected upstream iterator tests per profile; 215 unrelated tests filtered |
| `updater-safe`, `updater-personal` | 0 | Each: five cached tests; signature suite reports three passes including optional artifact early return; five portable archive/confinement tests with one ignored dispatcher; one Unix ZIP9 symlink-refusal contract |
| `root-gate` | 143 | Interrupted after overlapping owner stress workload; partial checks are not a whole-gate pass |
| `root-gate-final` | 143 | Interrupted while waiting behind owner follow-up build; not a pass |
| `root-gate-released` | 101 | Reached Clippy after provenance/25 controls/diagnostics/formatting; owner changed predict.rs during run, exposing private LLM_TIMEOUT access and Duration/RerankBounds mismatch; source inventory changed, locks and sibling identities/diffs stable |
| `desktop-gate-released` | 143 | 100 Routes assertion tests passed; codegen stage interrupted while owner edits continued; no desktop-gate pass |
| `root-gate-owner-released` | 0 | All four runtime cohorts, warning-denied Clippy/docs, 272 repository consistency tests, codegen, boundary, installer coexistence and rollback passed; concurrent test-only edit prevents a frozen-source whole-gate claim |
| `desktop-gate-owner-released` | 0 | 100 Routes assertions, 178 frontend tests, typecheck/build/bundle checks, both edition Rust suites, scratch daemon and linked binary passed; captured source/pins/siblings unchanged |
| `delta-format` | 0 | Package-scoped formatting check on final test bytes |
| `delta-clippy-default`, `delta-clippy-wdbx`, `delta-clippy-personal-edition`, `delta-clippy-accel` | 0 | Test targets included; warnings denied in each cohort |
| `delta-clippy-default-stable` | 0 | Default test-target Clippy rerun after WDBX commit; captured Abbey source/pins and final sibling state unchanged |
| `delta-tests-default`, `delta-tests-wdbx`, `delta-tests-personal-edition`, `delta-tests-accel` | 0 | Nine prediction-producer tests per cohort, 36 case executions total; final source/pins stable throughout each follow-up |
| `audit-final`, `vendor-audit-final`, `provenance-complete` | 0 | Refreshed consumer policy scan, no-ignore vendor audit and full final provenance/resolution check |
| `consumer-strict-audit-final` | 1 | Exactly the two existing bincode/paste maintenance warnings, no ignores |
| `completion-coverage`, `completion-final` | 0 | Ledger coverage checks only |
| `provenance-final`, `instructions-final` | 0 | Final full source/resolution verifier and instruction consistency checks |
| `sibling-pins` | 2 | Unsupported --help probe; usage error, no sibling gate executed; corrected --print-pins probe passed |

Vendor runtime/iterator, clone and updater receipts have unchanged captured
source and lock inventories. Macro diagnostics and subclass expectations were
not overwritten. Existing upstream warnings remain visible. The source inventory
covers src, tests, tools, desktop Rust source/tests and vendor; Git diff/status
and explicit manifest/lock hashes supplement it. Early receipts preceding that
expanded inventory must not be read as complete untracked-source inventories.

The mandatory consumer scan passes under its existing policy exceptions; the
no-ignore consumer audit still fails. The ledger records these separately and
retains historical baseline and candidate failures. Overall remediation remains
`in_progress`. No distribution-wide license, new secret scan, CI execution or
pinned-sibling gate is claimed by this packet.

## Final source boundary and review

The user explicitly released the owner after the earlier interruptions. The
completed root gate ran for 70.1 minutes and exited 0, with unchanged captured production
sources, dependency pins and captured sibling identities/status/diffs. Its full
inventory nevertheless changed: `src/tui/tests/prediction_producer.rs` plus two
generated Python cache files. The root source inventory includes caches, so
those incidental byte changes remain visible in the unmodified raw receipt.

The prediction test's observed modification time was 13:53:42 UTC, after the
root default/WDBX/personal library-test artifact times of 13:30:59, 13:40:03 and
13:47:25 UTC. The accelerator artifact time was 13:54:00 UTC. These filesystem
observations are retained in private `test-delta-timing.json`; they are not
immutable build provenance. The passing root command cannot be presented as
all four whole-gate test artifacts exercising the final test bytes.

Independent review recommended a composed evidence boundary: preserve the root
exit 0 and separately verify the final test-only delta, without repeating
unchanged installer checks. Package formatting, Clippy including test targets,
and nine prediction-producer tests now pass in each of default, WDBX, personal
and accelerator cohorts. Every follow-up has unchanged captured source/pins;
the complete desktop gate also passes with unchanged source/pins/sibling state.
This is composed local qualification evidence, **not a frozen-source whole-gate
pass**. Original failed/interrupted attempts and all source differences remain.

During `delta-clippy-default`, WDBX advanced from dirty
`612d3570f9ab0d842519aa4dbc4cb977aa33c9e0` to clean
`e5fd43bbfa901e7928a7c704dad43afee9a9efc3`. The original before/after state remains
in its raw receipt. A separate `delta-clippy-default-stable` rerun now passes
with unchanged final sibling state. The remaining focused cohorts ran against
the committed sibling. Root/desktop whole-gate execution evidence belongs to
their earlier captured sibling state; it does not qualify the final WDBX commit
as independently whole-gate tested. No byte-equivalence proof for all earlier
untracked sibling inputs or CI-pinned reproduction is asserted.

Abbey HEAD also advanced externally to
`8fdb4ed5f3a869726354bb89a8925f5e96aeaa00`, a documentation-only commit containing
the earlier report, published summary and ledger. Its exact three-file diff has
no production or lock changes. This task made no commit and preserved that
history; receipts retain the identity transition.

Final independent review verified all 48 published receipt/output hashes and
exits, fixture counts, source/sibling boundaries, ledger consistency and
publication portability. It found no blocking or should-fix issue and recommends
retaining this completed scoped evidence packet with current pins preserved.
Broader remediation remains `in_progress`: native acceptance, no-ignore consumer
audit, CI reproduction and frozen-source whole-gate qualification remain open.

## Platform and promotion limits

Native Linux GTK3 CompositeTemplate/TemplateChild and Linux application acceptance
remain UNEXECUTED. Native Windows ARM64/x64 hostile ZIP extraction, junction and
symlink contracts, real safe/personal NSIS/MSI update, restart, edition-state
preservation and interrupted/failed-update recovery remain UNEXECUTED. Positive
installer acceptance requires the existing disposable-VM operator adapter.

The installed x86_64-pc-windows-msvc Rust target does not establish Windows
compilation or execution. Windows-only bodies remain untypechecked by this
packet; PowerShell is unavailable and its runner remains unexecuted. Optional
built-artifact signature verification is not exercised without an artifact.
POSIX mode bits, Windows ACLs and same-user temp-directory races retain their
previous evidence gaps. No platform skip or early-return artifact check counts
as native acceptance. Both GNU cross-target guards skipped because those targets
were absent; neither checked the installed Windows MSVC target. Optional desktop
Routes GUI, packaged-app and menu/icon acceptance flags were not enabled. The
local installer smoke verifies accelerator edition coexistence and rollback;
it does not establish a signed/notarized production desktop release.

The mandatory consumer scanner retains exactly its existing bincode
RUSTSEC-2025-0141 and paste RUSTSEC-2024-0436 exceptions. Vendor strict scanning
uses no advisory ignores. No new suppression, pin promotion, release publication, deployment, checkout
switch, task-authored commit or task-authored sibling source modification is
included.
Overall completion/platform claims remain open.
