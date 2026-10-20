# Retained Rust finishing receipt — 2026-10-20

Status: the isolated bounded Rust finishing snapshot passed the authoritative
root gate (exit 0, final `check.sh: OK`) with unchanged complete Abbey/ABI/WDBX
source/index manifests. Concurrent shared working-tree changes remain
unqualified. The broader completion program remains in progress. Exact pins
remain authoritative under the [rewrite decision](../../REWRITE-DECISION-2026-10-19.md).
No production defect was demonstrated or repaired. No commit to main or
publication is part of this work.

## Successful isolated qualification

`sh ./check.sh` ran with verified PATH selecting cargo 1.100.0-nightly
(e8cb624d5 2026-08-22) and rustc 1.100.0-nightly (0dfb098f3 2026-08-31).
All four modes passed: 932 default, 948 WDBX, 923 personal-edition and 944
accelerator Rust test executions (3,747 overlapping executions, zero failures
or ignored tests). All four warning-denied clippy and private-item rustdoc
checks, 82 Python tests, five desktop codegen checks, claims/instruction checks,
Program 3 boundary and staged installer verification/relocation rollback passed.
Soft source warnings remain for doctor (836 lines) and registry (818 lines).
Windows GNU and Linux GNU cross targets were absent and skipped. Release builds
emitted the existing rust-objcopy `@rpath/libLLVM.dylib` warning; scratch install
and rollback still passed. No compiler pins changed.

The detached worktree at
`/Users/donald/.codex/worktrees/rust-finishing-qualification/abbey` starts from
Abbey `f81e2ec` and copies only this task's paths. Adjacent ABI is clean local
HEAD `cf3d9a425560a8223f476663ec45551d81efcf2c`; initial dirty ABI kernel/lib,
toolchain and hook work is excluded and is not qualified here. WDBX starts at
`08a69b3f22db61093e06527b3308b10589687022` plus the exact initial formatting-only
`cpu.rs` delta (file SHA-256
`ed8c872ca71dbf097ae42e020310d0f0123a516257c7b66c63e7bb6809a48ca6`).
No shared sibling work was edited. Complete before/after manifest digests agree:

| Snapshot | SHA-256 |
| --- | --- |
| Abbey | `51dbc57f168cbc22289d49dc1c2ee8af495f5b824860fa534083343e6e31a732` |
| ABI | `bf003ee6c8dafa644856210786d85a69a5a263b963ce85b92412134470632e45` |
| WDBX | `c429d1da6917e568eedbccc773e0c1907e120df406ec1b2faea34316acb719c1` |

This receipt and the two reconciliation sections were refreshed after the
frozen successful gate. Separate scoped documentation checks must qualify
that doc-only delta; it is not part of the frozen gate's source digest.

## Scope and focused evidence

Test-only additions in `tests/learn_distill.rs` exercise independently reopened
CLI learning lifecycle for SQLite and feature-gated WDBX. Isolated HOME/state
and owned PATH teacher fixtures avoid user data and live services. WDBX is the
primary end-to-end acceptance backend: tests require its directory and reject
`memory.sqlite`, preventing silent fallback. SQLite remains compatible.
Successful tests correlate receipt/durable/SFT IDs, exact teacher identity and
provenance, unreviewed status, prompt/response roles, and exact invalidation
followed by reopened review/SFT exclusion. Partial-output failure, empty success
and external SIGTERM yield no success receipt and no reopened SFT pair. This is
not a raw-store row-count assertion; existing store-before-receipt tests provide
separate persistence-ordering evidence. External interruption is distinct from
the audited cooperative TUI cancellation evidence.

`tools/tests/smoke_tui_slash_pty.py` extends selected-ABI `/learn review` and
`/learn sft` coverage. Its private SQLite fixture seeds a pair, removes FM,
selects ABI, and requires output retained across resize, unchanged provider
receipts and exact chat-id bytes. It does not compare every runtime row or
claim WDBX TUI acceptance. Both independent reviews passed with optional,
nonblocking assertion-precision improvements.

All commands below used `rustup run nightly-2026-09-01 cargo` and exited 0:

| Arguments | Passed |
| --- | ---: |
| `test --locked --features wdbx --test learn_distill` | 10 |
| `test --locked --test learn_distill` | 7 |
| `test --locked --lib tui:: -- --test-threads=1` | 105 |
| `test --locked --lib agent:: -- --test-threads=1` | 59 |
| `test --locked --lib capture::tests:: -- --test-threads=1` | 1 |
| `test --locked --test tui_pty --test tui_slash_pty --test tui_metadata_pty --test tui_local_help_notice --test tui_missing_peer_notice --test tui_please_fix_notice -- --test-threads=1` | 15 |
| `test --locked --test tui_slash_pty local_slash_uses_actual_selected_backend_and_conversation_owner -- --exact --test-threads=1` | 1 repeat |

Pinned rustfmt and Task 1 whitespace checks also passed. Initial focused
attempts encountered feature-build artifact concurrency contamination and an
overly broad interruption fixture; narrowing the fixture to `fm respond` and
serialized learning reruns resolved those attempts. Focused results do not
replace the authoritative root gate or establish all-feature qualification.

Reports and independent reviews are retained in the stable external `reports/`
directory beneath the receipt root below, rather than depending on scratch
implementation-workspace paths.

## Authoritative root attempts

Full logs, complete source/index manifests, result JSON and gate runner remain
outside the checkout at
`/Users/donald/.codex/verification/abbey-rust-finishing-20261020`.

| Receipt prefix | Command/compiler | Result | Source stability |
| --- | --- | --- | --- |
| `root-first` | `rustup run nightly-2026-09-01 sh ./check.sh`; inherited PATH selected unintended Homebrew compiler | Exit 1, formatting failed | Abbey/ABI/WDBX unchanged |
| `root-pinned` | `sh ./check.sh` with verified pinned PATH: cargo 1.100.0-nightly (e8cb624d5 2026-08-22), rustc 1.100.0-nightly (0dfb098f3 2026-08-31) | Exit 101; formatting/clippy passed, compilation failed with missing `src/runtime/store/governed.rs` during concurrent unrelated Abbey edits | Abbey/ABI changed; WDBX unchanged |

The pinned `root-retry` also exited 1 on formatting while ABI changed. The
`root-remainder` diagnostic exited 101 amid incomplete concurrent runtime
modules; it was not an authoritative full gate. Initial isolated `isolated-root`
exited 1 on committed WDBX `cpu.rs` formatting, before restoring the verified
initial formatting-only delta described above. All failed attempts remain
retained; they do not qualify shared WIP. The subsequent `isolated-qualified`
result is the authoritative exit-zero snapshot receipt.

No desktop gate is required for this test-only change because shared contracts
and desktop behavior did not change; this does not qualify concurrent changes.
Synthetic fixture success proves dataset generation/curation and bounded local
runtime behavior only. Fine-tuning, weight updates, production weights,
provider-neutral owned runtime, packaged desktop, live teacher quality and
Linux/Windows runtime retain their existing Proposed or separately unverified
boundaries. Generated capability statuses and counts remain unchanged.
