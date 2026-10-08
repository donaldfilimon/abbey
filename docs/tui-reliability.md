# Chat-first TUI reliability evidence

Status: Current for the bounded macOS/Unix fixture reliability slice.
The complete production gate passed on 2026-10-02 with nightly-2026-09-01.

## Implemented behavior

- Terminal modes are guarded across partial entry, errors, suspension, normal
  exit, and panic unwinding. Cleanup attempts every reset; the original loop
  error takes precedence over shutdown or terminal-cleanup errors.
- A run owns its worker handle and cancellation token. Local slash commands and
  explicitly confirmed allowlisted OS commands execute on supervised workers.
  Quit cancels and joins the worker after process-group teardown. A missing
  completion or worker panic becomes a failed run. Unix executor-version
  probes have a 500 ms deadline and 4 KiB per-stream capture limits.
- A failed or interrupted run discards queued prompts with a visible count.
  Successful queue advancement preserves the unsent composer text and cursor.
- Unix streaming accepts at most 4 MiB raw stdout, 4 MiB decoded payload, and
  65,536 decoded events per run, reserving two event slots for a bounded
  failure notification and exactly one canonical run-completion event.
  Exceeding a limit cancels or tears down the
  executor and cannot commit a successful local transcript. The UI applies at
  most 128 events per tick and drains accepted events before finalization.
- Transcript retention is 2,000 cells and 8 MiB payload, with a 4 MiB cell
  ceiling and UTF-8-safe clipping. Old completed cells are removed first and
  retention is visible. Active tool-cell exhaustion fails the run explicitly.

## Verification

The deterministic tests live alongside the TUI, stream tap, streaming agent,
and Unix supervisor. `tests/tui_pty.rs` executes
`tools/tests/smoke_tui_pty.py` against the built Abbey binary, using isolated
configuration, state, workspace, editor, and ABI fixture executors.

The real PTY harness passed these nine scenarios on macOS on 2026-10-02:
multiline bracketed paste without submission, incremental output before the
executor exits, resize, successful editor return, terminal-attribute and mode
restoration, missing-editor recovery, interrupt with queued-prompt discard,
descendant teardown, and quit while running with teardown before return.

The harness provides cursor-position responses required by crossterm and
checks rendered terminal cells rather than treating ANSI diff output as text.
It uses the standard Python library and does not call any model service.

The final command was `CARGO_INCREMENTAL=0 rustup run nightly-2026-09-01 sh ./check.sh`
and returned exit 0. Passed Rust test totals were 850 default, 864 WDBX,
841 personal-edition, and 862 accelerator (overlapping feature suites).
All four warning-denied clippy and private-item rustdoc checks passed, as did
82 Python tests, claim projections, instruction consistency, desktop codegen
drift, the Program 3 boundary, and isolated accelerator install/rollback checks.
The claim registry exceeds the 800-line soft warning threshold (818 lines);
the hard source-size guard passes. Linux and Windows cross targets were absent
and skipped; this is not cross-platform runtime acceptance.

## Evidence boundary

These are macOS/Unix fixture and automated proofs, not live vendor-service,
model-quality, Windows runtime, desktop, daemon, or owned tool-host acceptance.
Non-Unix local TUI capture explicitly refuses because bounded cancellation is
unavailable there; non-Unix backend output is buffered, not live-streamed.
The independent learn-distill working-tree changes remain separate.

## 2026-10-03 shared-source review correction

The 2026-10-02 gate above remains historical evidence. The subsequent complete
shared-diff review includes the separate learning/distillation edits. Confirmed
repairs preserve selected backend and conversation state across local commands,
join nested captures and metadata probes before cancellation completes, fence
cumulative capture budgets before durable turn writes, and issue distillation
IDs only after storage commits. Completion freshness, Unicode cursor boundaries,
queued submission, bounded pickers/panels, and owned prediction also have
attributable regression evidence. Capture warnings and generation advice now
reach the original notice tap and survive redraw; ordinary CLI warnings retain
their stderr behavior.

The preliminary full gate passed, but final source attribution remains pending
until independent complete-diff review, authoritative root and applicable desktop
gates, and unchanged complete Abbey/ABI/WDBX manifests agree. Actual attempts and
final source-only receipts live outside the checkout under
`/Users/donaldfilimon/.codex/verification/abbey-bot-continuity-20261003/`.
Automated PTY and owned scratch-daemon fixtures do not establish live vendor wire
formats, model quality or training, installed artifact identity, a native GUI,
managed services, Linux/Windows runtime, or human acceptance.
