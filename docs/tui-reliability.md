# Chat-first TUI reliability evidence

Status: Current for the bounded macOS/Unix fixture reliability slice.
The complete production gate passed on 2026-10-02 with nightly-2026-09-01. The
2026-10-22 reliability cohort is the behaviour described below; it is receipted
in [verification/2026-10-22-tui-reliability-cohort.md](verification/2026-10-22-tui-reliability-cohort.md).

## Implemented behavior

- Terminal modes are guarded across partial entry, errors, suspension, normal
  exit, and panic unwinding. Cleanup attempts every reset; the original loop
  error takes precedence over shutdown or terminal-cleanup errors.
- The panel refresh — doctor, memory, skills, personas, routes, models, and
  history — runs on a background worker, and its result is applied on the UI
  thread when it lands (`src/tui/refresh.rs`). The doctor's `--version` probe,
  the model list, and the memory backend's open no longer run in the frame the
  user is reading the answer on. A refresh in flight is never joined; a newer
  refresh replaces it and the stale result is discarded.
- The clipboard copy runs on a worker and its poll is tri-state: Pending,
  Finished(outcome), Failed(reason) (`src/tui/clipboard.rs`). A worker that
  dies without an answer is reported rather than left as "copying…" for the
  rest of the session, and the shutdown join is bounded, so a hung clipboard
  command cannot hold the terminal in raw mode.
- An interrupt is a request, not a promise. The app waits at most three
  seconds and then gives up: the worker is abandoned and detached, exit 130 is
  reported, and the transcript says the app gave up. The activity row says
  "giving up in Ns" while it waits.
- The turn-end repaint no longer asks the terminal where the cursor is
  (`src/tui/run_loop.rs`). `Terminal::clear` issues `ESC[6n` and waits up to
  two seconds, which stalled the frame and ended the event loop on a terminal
  or multiplexer that never answers. The reset is a viewport resize now, and a
  failed reset is reported in the status line instead of ending the loop.
- Theme preview and theme choose are different acts. Ctrl-T cycles the palette
  without persisting it; choosing a palette in the palette overlay writes it
  to the state directory, so a chosen palette wins over the terminal's
  reported brightness in every later session.
- There is a light palette (`daylight`) for a light terminal profile, and every
  palette is checked against the WCAG contrast ratio — 4.5:1 for body text,
  3:1 for borders — so a palette that stops being legible fails the build
  instead of shipping (`src/tui/theme.rs`).
- Esc-Esc recalls the last prompt only when the draft has not changed since
  the first Esc; a draft typed in between survives.
- A run owns its worker handle and cancellation token. Local slash commands and
  explicitly confirmed allowlisted OS commands execute on supervised workers;
  Quit cancels and joins the worker after process-group teardown; a missing
  completion or worker panic becomes a failed run. Unix executor-version
  probes have a 500 ms deadline and 4 KiB per-stream capture limits.
- A failed or interrupted run discards queued prompts with a visible count,
  and a successful queue advancement preserves the unsent composer text and
  cursor.
- Unix streaming accepts at most 4 MiB raw stdout, 4 MiB decoded payload, and
  65,536 decoded events per run; exceeding a limit cannot commit a successful
  local transcript, and the UI applies at most 128 events per tick.
- Transcript retention is 2,000 cells and 8 MiB payload, with a 4 MiB cell
  ceiling; active tool-cell exhaustion fails the run explicitly.

## The startup appearance query, and its two residual costs

The TUI asks the terminal for its default background (`OSC 11`) before the
first frame, parses the reply, and follows the terminal's own appearance when
no palette has been chosen (`src/tui/terminal_appearance.rs`). The query is
read-only, bounded, and parsed, never executed. Two costs remain, and they are
costs rather than guarantees:

- A keystroke typed inside the query window is lost: the answer arrives on the
  terminal's input queue, and that queue cannot be put back.
- A terminal that answers after the grace has its reply read as ordinary key
  events, so the reply bytes land in the composer. Once the bytes are in the
  event queue the app cannot tell an `OSC` reply from typed text.

## Keys, signals, and the completion menu

- The TUI installs no SIGINT or SIGTERM handler. An external kill therefore
  leaves the terminal un-restored — raw mode and the alternate screen stay up.
  The in-process Ctrl-C path is a key event rather than a signal: raw mode
  means the terminal generates no SIGINT, and Ctrl-C clears the composer
  first and quits on the second press.
- Enter accepts a completion the user asked for and nothing else: a path
  typed after `@`, a command name being typed after `/`, or a suggestion the
  user moved the menu selection onto. A menu the ranker opened on ordinary
  prose is none of those, so Enter submits the draft instead of turning the
  sentence into a slash command. Tab accepts whatever the menu holds, and
  during a run Enter accepts an open completion rather than queueing the
  half-typed token.

## Verification

The deterministic tests live alongside the TUI, stream tap, streaming agent,
and Unix supervisor. `tests/tui_pty.rs` executes
`tools/tests/smoke_tui_pty.py` against the built Abbey binary, using isolated
configuration, state, workspace, editor, and ABI fixture executors; it checks
rendered terminal cells rather than treating ANSI diff output as text.

The 2026-10-22 cohort is proven by unit tests in `src/tui/`, run with
`./tools/cargo.sh test --locked -p abbey --lib -- tui::`:

| Behaviour | Test |
| --- | --- |
| The refresh does not block the UI thread | `tests::background::a_slow_refresh_does_not_block_the_ui_thread` |
| An interrupt that never ends is given up on | `tests::background::an_interrupt_that_never_ends_is_given_up_on` |
| A clipboard worker cannot hold the join or the shutdown; a dead one is reported | `clipboard::tests::a_worker_that_never_answers_does_not_block_the_join`, `clipboard::tests::a_worker_that_dies_without_an_answer_is_reported`, `run_loop::tests::a_clipboard_worker_that_never_answers_does_not_block_the_shutdown` |
| The turn-end reset asks no cursor-position query, and a failed reset does not end the loop | `run_loop::tests::the_screen_is_reset_only_when_a_run_has_just_ended`, `run_loop::tests::a_failed_screen_reset_does_not_end_the_loop` |
| Preview does not persist; choosing does | `tests::app_state::cycling_the_theme_previews_it_without_choosing_it`, `tests::app_state::choosing_a_palette_in_the_overlay_saves_it` |
| Every palette is legible | `theme::tests::every_palette_is_legible`, `theme::tests::the_daylight_palette_is_readable_on_a_light_ground` |
| Esc-Esc keeps a draft typed in between | `tests::keymap::esc_esc_does_not_destroy_a_draft_the_user_typed_in_between` |
| Enter accepts an open completion during a run | `tests::keymap::enter_accepts_the_open_completion_while_a_run_is_active` |
| The appearance reply is parsed, bounded, and never executed | `terminal_appearance::tests::` (25 tests) |

## Evidence boundary

These are macOS/Unix fixture and automated proofs, not live vendor-service,
model-quality, Windows runtime, desktop, daemon, or owned tool-host
acceptance. Non-Unix local TUI capture refuses because bounded cancellation is
unavailable there; non-Unix backend output is buffered, not live-streamed.

## 2026-10-03 shared-source review correction

The 2026-10-02 gate remains historical evidence. The subsequent shared-diff
review included the separate learning/distillation edits: repairs preserve
selected backend and conversation state, and fence capture budgets.

## 2026-10-20 retained Rust focused acceptance

The bounded captured finishing cohort is complete: full root and separate
desktop gates passed against identical unchanged Abbey/ABI/WDBX manifests.
See the [receipt](verification/2026-10-20-rust-finishing.md) for commands,
source stability, and the remaining paired-gate requirements.

## 2026-10-22 TUI reliability cohort

The panel refresh moved off the UI thread, the clipboard poll became
tri-state with a bounded shutdown join, an interrupt gained a bounded grace,
the turn-end repaint stopped asking the terminal where the cursor is, theme
preview and theme choose became separate acts, a light palette arrived with
contrast checks, and Esc-Esc stopped destroying a draft typed in between. The
residuals are recorded in the
[receipt](verification/2026-10-22-tui-reliability-cohort.md).
