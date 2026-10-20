# Multi-agent CLI/TUI/learning follow-up — 2026-10-20

This follows the [TUI/FM slice](2026-10-20-tui-fm-learning.md). Work was split
between voice, learning, model/command parity, terminal-regression, and independent
review agents, with integration and validation in the canonical Rust checkout.
Existing dirty learning-output and Doctor/TUI presentation work was preserved.

## Bounded changes

- Whisper receives an explicit output basename in a unique owned workspace.
  Transcripts and recordings are cleaned up by RAII on ordinary success/error
  paths; supplied WAVs and neighboring user files are never adopted or deleted.
  Recording filenames are no longer process-ID-only. Cleanup remains best-effort
  and cannot be guaranteed after forced process termination.
- Learning rejects malformed counts, unknown/duplicate improve flags, extra
  arguments, and blank corrections before writes. Apply uses the proposed route
  snapshot, reports retained progress on partial failure, and preserves training
  candidates and provenance. Absent-store reads and empty route ingestion do not
  create a store. Existing-store opening can still perform migration/recovery;
  route deduplication remains bounded to 10,000 live stored route records.
- TUI startup preserves the already-resolved model, including explicit CLI
  overrides. CLI and slash model selection share backend-specific normalization
  and literal persistence; ABI catalog IDs avoid Cursor expansion. FM uses exact
  `system`/`pcc` vocabulary. Model inspection reports the live selection.
- Slash model inventory and status use the same backend-specific inventory and
  refusal rules as CLI commands instead of sending foreign provider verbs.
- A full-width hard newline no longer inserts a cursor-dependent blank row.
- CLI long help describes selectable providers, local-command independence,
  FM's on-/off-device distinction, and the unchanged OS-control allowlist.

## Focused evidence

- Portable voice: 10 tests passed with fake executors, no microphone/model use.
- Learning: 19 default and 20 WDBX focused tests passed, including retained
  provenance/candidate checks.
- Shared model selection/inspection: four focused tests passed; TUI startup and
  provider command tests also passed in their focused runs.
- Composer/rendering: eight focused tests passed.
- The provider PTY suite passed five real terminal scenarios with six actual
  private fake-FM calls: provider switch plus local help, explicit startup model,
  queued deliberate PCC selection, clear back to on-device system despite ambient
  PCC, and failed model persistence discarding subsequent queued generation.
  These are argv/lifecycle fixtures, not calls to Apple PCC or any model service.
- Independent read-only integration review reported no concrete blockers in the
  combined source and tests. That review did not run builds or tests and is not
  substituted for the production gate.

No bot repository was created, no training or model download was introduced, and
no shell/tool safety restriction was relaxed. Bot work remains deferred per the
earlier user choice. This does not close Proposed runtime or training goals.

## Production gate

Final isolated `./check.sh` validation passed with `check.sh: OK`: all four
feature-mode lint/test/private-rustdoc stages, Python checks, claims/instruction
and desktop IPC drift checks, Program 3 boundary, and isolated installer
verification/rollback completed. Windows/Linux GNU cross-checks were skipped
because the targets were not installed; sibling and soft size warnings remain.

Log: `/private/var/folders/c_/thtfn_7d1p9378rxsp4pwz6c0000gn/T/opencode/abbey-multi-agent-followup-gate.log`.
This is local gate evidence, not frozen-source, full desktop-product,
cross-platform runtime, live voice, or training qualification.
