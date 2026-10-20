# Retained Rust finishing receipt — 2026-10-20

Status: **COMPLETE for the bounded captured Rust finishing cohort.** Full root
and separate desktop gates both exited 0 against the same unchanged complete
Abbey/ABI/WDBX source/index cohort. The broader completion program remains
in_progress, and later canonical producer changes are separately unqualified.
Exact nightly pins remain authoritative under the
[rewrite decision](../../REWRITE-DECISION-2026-10-19.md); retained Zig evidence
is preserved. No commit or publication is part of this work. Historical and
intermediate attempts below remain evidence for their own snapshots.

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

Learning acceptance additions in `tests/learn_distill.rs` exercise independently reopened
CLI learning lifecycle for SQLite and feature-gated WDBX. Isolated HOME/state
and owned PATH teacher fixtures avoid user data and live services. WDBX is the
primary end-to-end acceptance backend: tests require its directory and reject
`memory.sqlite`, preventing silent fallback. SQLite remains compatible.
Successful tests correlate receipt/durable/SFT IDs, exact teacher identity and
provenance, unreviewed status, prompt/response roles, and exact invalidation
followed by reopened review/SFT exclusion. Partial-output failure, empty success
and external SIGTERM yield no success receipt, no reopened SFT pair, and no
raw train-candidate export entries. The later raw-candidate assertions close the
earlier empty-SFT-only evidence gap; existing store-before-receipt tests provide
separate persistence-ordering evidence. External interruption is distinct from
the audited cooperative TUI cancellation evidence.

`tools/tests/smoke_tui_slash_pty.py` extends selected-ABI `/learn review` and
`/learn sft` coverage. Its private SQLite fixture seeds a pair, removes FM,
selects ABI, and requires fresh assistant output, successful child completion,
exact seeded review/SFT strings retained across resize/redraw, unchanged provider
event count and exact chat-id bytes. It does not compare every runtime row or
claim WDBX TUI acceptance. Later assertions address the earlier optional SFT-content precision suggestion;
exact CLI JSON structure and content remain independently covered.

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

## Current canonical attempts and scoped repairs

Later finishing work includes stronger raw-candidate and PTY content assertions,
a `desktop/Cargo.lock` synchronization of local path dependency edges (including
`abi-capability`, with no registry upgrade or checksum replacement), relocation
of unchanged `tool_approval::create_on` before its test module to satisfy clippy,
and a strict legacy migration fixture expectation correction from schema 7 to 8.
The helper's scope, visibility and body are unchanged. The schema fixture still
requires exact version equality and retains backup, privacy and reopen checks.
A later strict fixture update from schema 8 to 9 tracks the subsequently
advanced migration set; it does not relax exact equality. These repairs do not
qualify concurrent ABI governed-adapter or migration policy.

All canonical commands use the pinned nightly reported above. Result JSON and
before/after manifests remain under the external receipt root; the table records
observed outcomes, not qualification inferred from a command name.

| Receipt prefix | Command | Result | Source stability |
| --- | --- | --- | --- |
| `canonical-root-01` | `sh ./check.sh` | Exit 1 | All three unchanged |
| `canonical-root-02` | `sh ./check.sh` | Exit 1 | ABI changed |
| `canonical-root-03` | `sh ./check.sh` | Exit 101 | ABI/WDBX changed |
| `canonical-root-04` | `sh ./check.sh` | Exit 101 | WDBX changed |
| `canonical-root-05` | `sh ./check.sh` | Exit 0, final `check.sh: OK`; 3,818 overlapping Rust executions | All three changed |
| `canonical-focused-01` | `cargo test --locked --features wdbx --test learn_distill --test tui_slash_pty` | Exit 0; 15 passed | ABI changed |
| `canonical-desktop-deps` | `bun install --cwd desktop --frozen-lockfile` | Exit 0 | All three unchanged; dependency installation only |
| `canonical-desktop-01` | `sh ./desktop/check.sh` | Exit 101 | ABI changed |
| `canonical-desktop-02` | `sh ./desktop/check.sh` | Full gate exit 0 | ABI/WDBX changed |
| `canonical-clippy-fix` | `cargo clippy --all-targets -- -D warnings` | Exit 0 | WDBX changed |
| `canonical-legacy-fix` | `cargo test --locked --test legacy_conversation_migration` | Exit 0; strict schema 7→8 fixture | All three unchanged; focused fixture only |
| `canonical-schema9-fix` | `cargo test --locked --test legacy_conversation_migration` | Exit 0; 1 passed; later strict schema 8→9 fixture | All three unchanged; focused fixture only |

`canonical-root-05` completed with 949 default, 968 WDBX, 940 personal-edition
and 961 accelerator passes (3,818 overlapping Rust executions). All four clippy
and private-item rustdoc modes, 82 Python tests, codegen checks and staged
installer/rollback checks passed. Soft warnings remain for doctor (836 lines),
registry (818), and governed/tests (821). Windows GNU and Linux GNU targets were
absent and skipped; release rust-objcopy's existing LLVM dylib warning remains.
All three complete manifests changed during this exit-zero attempt. Neither it,
the drifting desktop exit-zero attempt, nor stable focused schema fixtures
established paired current qualification at that stage. The final captured gates
below subsequently meet that requirement. Qualification requires one
unchanged complete Abbey/ABI/WDBX source/index cohort, including relevant
untracked inputs and modes, with producer revision/content/corpus/contract scope.
Record counts, skips and limitations from those results. Root IPC codegen alone
does not qualify desktop. Any later receipt-only delta needs scoped documentation
checks and its own recorded delta; preserve the frozen gate manifests.

Synthetic fixture success proves dataset generation/curation and bounded local
runtime behavior only. Fine-tuning, weight updates, production weights,
provider-neutral owned runtime, packaged desktop, live teacher quality and
Linux/Windows runtime retain their existing Proposed or separately unverified
boundaries. Generated capability statuses and counts remain unchanged.

## Final captured current-cohort qualification

The complete dirty source/index cohort was faithfully captured in the native
worktree `/Users/donald/.codex/worktrees/rust-finishing-current-cohort`, including
current WDBX `429318` plus its dirty inputs (not the older ABI-side `2969`
snapshot) and the exact schema-9 producer source. Independent capture audit
passed. This qualifies that captured cohort, not later producer changes.

`current-cohort-root-final-result.json` and
`current-cohort-desktop-final-result.json` both record exit 0 and identical
before/after complete source/index manifests for all three repositories:

| Repository | Final captured manifest SHA-256 |
| --- | --- |
| Abbey | `bc5ff6325d9196c9e89288977d37303c3ffb4038eac9b22ec2a1d418758fd1c6` |
| ABI | `2a8bf2f00030e28746f73e9a63a51fd3db97fac224066133d951d2c023327aab` |
| WDBX | `9bd04df7dff86cdbce20b9b30098e5e0e74e73c287e9380b4311c1c238f97243` |

The root gate passed 950 default, 974 WDBX, 941 personal-edition and 962
accelerator tests (3,827 overlapping Rust executions), all four clippy and
private-item rustdoc modes, 82 Python tests, five desktop codegen checks, and
actual copied-layout installer verification plus rollback. The separate desktop
gate passed on those exact same manifests. Both use the retained pinned nightly.
Soft warnings remain for doctor (836 lines), registry (818) and governed tests
(821); Windows/Linux GNU targets were absent and skipped. Existing release
rust-objcopy SIGABRT from missing `@rpath/libLLVM.dylib` remains: this proves
neither clean stripping nor stable-candidate packaging. No compiler pin migrated.

An earlier captured failure in socket readiness was traced to a controlled fork
retaining a listener. The final test-only repair uses one queued stream with a
two-second read timeout and accepts only EOF/reset as closure evidence. Review
and the actual focused run passed with all three manifests stable. The earlier
repeated-connect repair was rejected because it lacked a backlog bound; its
superseded gate stopped with status -15. Failed and superseded receipts remain
retained and are not replaced by the final results.

The bounded finishing pass is closed. Receipt reconciliation changes only these
three documentation paths after the frozen gates; parent-scoped documentation
checks and an explicit delta qualify that reconciliation without overwriting
frozen manifests. The broader program stays in_progress. Further producer or
stable-compiler qualification requires its own authorized cohort and gates;
training, production weights, live services, packaged desktop and separate-host
runtime boundaries above remain unchanged.
