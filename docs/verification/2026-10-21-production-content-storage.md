# Production continuation: durable v4 content — 2026-10-21

This is a bounded implementation receipt for production-plan Task 2, not full
Abbey or desktop qualification. The complete production specification and master
plan remain governing. Rust 1.99.1 and qualified dependency pins are unchanged;
Zig remains canceled. No capability has been promoted.

## Implementation

- Turn admission no longer inserts `[admitted]` as provider text. Lifecycle state
  remains in the existing typed turn receipt and run events. Admission checks
  sequence capacity atomically before creating a new run; idempotent retries
  still return their existing receipt.
- Assistant previews retain a UTF-8 prefix of at most 16 KiB and count all actual
  text bytes, including later deltas after truncation. More than 4 MiB of turn
  text is rejected atomically. Preview write failure rolls back its content event.
- Content pages account for serialized JSON escaping and stay inside the existing
  512 KiB page contract. The existing storage byte-count column counts text bytes
  rather than JSON escape overhead. No schema migration or wire-shape change.
- Corrupt/missing content payloads fail reads rather than inventing a recovery
  notice. Nonzero cursors must belong to the exact conversation and run.
- Finalization cannot substitute a different terminal outcome. Lifecycle/reopen
  completeness flags describe the preview's actual byte coverage. The final
  conversation sequence slot is reserved for a terminal marker.

## Verification

Regression tests were observed failing before their corresponding fixes for
synthetic admission text, truncated byte totals/reopen, preview rollback,
cumulative output budget, escaped-content pages, corrupt payload reads,
terminal outcome substitution, and exhausted-conversation admission. Additional
coverage binds cursors to their exact run. Existing second-turn sequence-gap
coverage now uses actual provider output rather than the removed fake marker.

- Final focused v4 suite: 60 passed, zero failed.
- Earlier full default library suite: 977 passed, zero failed; this preceded the
  final admission-capacity regression and repair, so it is intermediate evidence.
- Desktop controller/model suite: 53 passed, zero failed.
- Desktop typecheck and generated IPC check: passed; five generated files current.
- Warning-denied default library Clippy: passed before final admission repair.
- Independent read-only review found the exhausted-admission regression; after
  its repair, final review reported no additional introduced correctness issues.
- Full desktop gate passed on the canonical checkout before provider chunking:
  both editions, scratch-daemon/no-fallback tests, native linking, frontend
  build/bundle checks and updater signatures. Opt-in rendered acceptance was
  not requested by this gate invocation.
- Full canonical root gate passed default and WDBX, then failed personal-edition
  Clippy on unused imports introduced by concurrent `src/config.rs` work. An
  after-run manifest comparison found only `src/config.rs` drift across the
  recorded Abbey/ABI/WDBX inputs. This is not a stable combined qualification.

An intermediate root attempt overlapped an unfinished test fixture and failed
compilation on a nonexistent test helper. The helper was corrected; this attempt
is retained as failed intermediate evidence, not a product qualification.

## Large provider reply continuation

The provider sink now splits a large provider text event into ordered UTF-8
chunks of at most 16 KiB. A fixture executes a reply crossing a multi-byte
boundary through ModelProviderExecutor, completes it, reopens storage, verifies
truncated-preview byte totals and reconstructs the exact output using single-event
pages. The fixture failed before chunking and passed afterward. Independent
read-only review found no introduced issues.

Concurrent configuration work subsequently broke canonical compilation, so this
continuation was checked in an isolated archive of Abbey HEAD plus only the
content-storage/provider implementation and test files, with symlinked unchanged
ABI/WDBX sources. The exact snapshot path is retained in the ignored execution
ledger. This deliberately excludes the other writer's configuration/desktop work;
it does not qualify the current combined checkout.

- Isolated final focused v4 suite: 61 passed, zero failed.
- Isolated final default library suite: 979 passed, zero failed.
- Isolated warning-denied default library Clippy: passed.
- Isolated WDBX, personal-edition and accelerator focused v4 suites: 61 passed,
  zero failed in each cohort. Warning-denied library Clippy passed in each.
- Formatting of all five implementation/test files and `git diff --check` passed.
  The five canonical files match the isolated candidate byte-for-byte.

## Remaining program

Task 1 source manifests and initial dirty-work inventory are captured in the
plan's ignored execution workspace. A concurrent writer's changes to desktop
`main.rs` are preserved. No immutable combined-source release is claimed here.
Task 2 remains open for the rest of its snapshot/search/crash/process acceptance.
Task 3 canonical multi-turn/provider/session integration remains the next
implementation dependency before broader task/tool authority.

The current source includes exact Stop/cancel contracts, client checks and UI;
earlier ledger statements that Stop does not exist are historical. This receipt
does not establish rendered Stop/descendant-teardown acceptance. Tasks, scoped
tools, automation, production models, training/media, native platform packaging,
and distributed proofs retain their existing unfinished boundaries. Missing
signing credentials, host/hardware proof, licenses and participant consent remain
separate prerequisites; no external action was taken by this continuation.
