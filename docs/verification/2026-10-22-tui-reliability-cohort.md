# 2026-10-22 — TUI reliability cohort

A verification receipt for the TUI reliability cohort: what changed, what is
now proven and by which test, what is still open, and what this receipt does
not prove.

The tree was **in flight** while this was written. Every citation below is a
snapshot read at 2026-10-22 03:55 EDT, and the timestamps of the runs are
given. `src/tui/keymap.rs` and `src/tui/predict.rs` were being edited during
the session (last written 03:39 and 03:38); the behaviour described here is
the one in those files at the snapshot, and it should be re-read if the cohort
is still being changed.

## What changed

### Landed on 2026-10-21

| Commit | Subject |
| --- | --- |
| `92974b2` | feat(tui): a light palette and a clipboard the host can actually reach |
| `088a819` | fix(tui): copy on a worker so a hung clipboard cannot freeze the app |
| `88f2e2e` | test(tui): prove the suspend path takes the terminal back |
| `4843f5e` | feat(tui): follow the terminal's own appearance when no theme was chosen |
| `b658bc9` | fix(tui): query the terminal inside the alternate screen |
| `88a748b` | test(tui): check every palette is readable, not just that it builds |
| `8ae377a` | test(tui): render markdown in every palette, and fix a doc link |
| `e67a64e` | test(tui): pin the key bindings |

### In flight in the working tree, uncommitted

`git diff --stat -- src/tui` at 03:55 EDT: **20 files changed, 1881
insertions(+), 362 deletions(-)**. Six files are untracked: `src/tui/command.rs`
and `src/tui/tests/{app_state,background,os_command,overlay_keys,prompt_submit}.rs`.

The seven behaviours, with the code that carries each:

1. **The panel refresh runs off the UI thread.** `RefreshWorker::start` spawns
   an `abbey-tui-refresh` thread (`src/tui/refresh.rs:50-59`); the collection
   is a pure function of the config and the state (`src/tui/refresh.rs:72`);
   `request_refresh` replaces any refresh in flight
   (`src/tui/refresh.rs:302-309`); `apply_refresh` writes the payload back on
   the UI thread (`src/tui/refresh.rs:321-335`), which the pump does
   (`src/tui/app.rs:572`). The refresh is never joined on the UI thread.
2. **The clipboard poll is tri-state, and the shutdown join is bounded.**
   `CopyPoll` is `Pending` / `Finished(CopyOutcome)` / `Failed(String)`
   (`src/tui/clipboard.rs:87-94`), produced by `poll`
   (`src/tui/clipboard.rs:136-144`); `join_within`
   (`src/tui/clipboard.rs:152-162`) waits at most `JOIN_GRACE` — 300 ms
   (`src/tui/clipboard.rs:35`) — and detaches a worker that misses it
   (`src/tui/app.rs:765`).
3. **An interrupt has a bounded grace.** `INTERRUPT_GRACE` is three seconds
   (`src/tui/app.rs:88`); the pump abandons a worker that has not reported by
   then and reports exit 130 with "the app gave up on it"
   (`src/tui/app.rs:610-621`); `RunHandle::abandon` detaches the thread
   (`src/tui/worker.rs:57-61`); the activity row counts down
   ("giving up in Ns", `src/tui/render.rs:390-399`).
4. **The turn-end repaint asks the terminal nothing.** The reset is a viewport
   resize (`src/tui/run_loop.rs:26-30`), not `Terminal::clear`, which issues
   `ESC[6n` and waits up to two seconds; it happens only when a run has just
   ended (`src/tui/run_loop.rs:37-39`), and a failure is reported in the
   status line rather than ending the loop (`src/tui/run_loop.rs:47-51`).
5. **Theme preview and theme choose are separate.** `cycle_theme` previews and
   says "(not saved)" (`src/tui/app.rs:310-314`); `select_theme` writes the
   palette to the state directory and says "(saved)"
   (`src/tui/app.rs:321-328`). Ctrl-T is bound to the preview
   (`src/tui/keymap.rs:129`); the palette overlay's `Theme: …` rows are the
   choice (`src/tui/overlays.rs:90-113`).
6. **There is a light palette, and every palette is measured.**
   `ThemeId::Daylight` (`src/tui/theme.rs:23`) with its own colours
   (`src/tui/theme.rs:234-253`); brightness by WCAG relative luminance
   (`src/tui/theme.rs:150-160`) and contrast by the WCAG ratio
   (`src/tui/theme.rs:170-176`), asserted for every palette
   (`src/tui/theme.rs:488-542`).
7. **Esc-Esc keeps a draft typed in between.** `EscWindow` carries the draft
   and cursor it saw (`src/tui/keymap.rs:21-39`), and the recall fires only
   when the draft is unchanged (`src/tui/keymap.rs:244-274`).

## What is proven, and how

All commands run from the repository root with the pinned toolchain
(`./tools/cargo.sh`, not a bare `cargo`).

| Behaviour | Test | Run |
| --- | --- | --- |
| The refresh collection does not run on the UI thread, and its result is applied | `tui::tests::background::a_slow_refresh_does_not_block_the_ui_thread` | green |
| An interrupt that never ends is given up on | `tui::tests::background::an_interrupt_that_never_ends_is_given_up_on` | green |
| A clipboard worker that never answers does not block the join | `tui::clipboard::tests::a_worker_that_never_answers_does_not_block_the_join` | green |
| A clipboard worker that dies without an answer is reported | `tui::clipboard::tests::a_worker_that_dies_without_an_answer_is_reported` | green |
| A hung clipboard worker does not hold the shutdown | `tui::run_loop::tests::a_clipboard_worker_that_never_answers_does_not_block_the_shutdown` | green |
| The screen is reset only when a run has just ended | `tui::run_loop::tests::the_screen_is_reset_only_when_a_run_has_just_ended` | green |
| A failed screen reset does not end the event loop | `tui::run_loop::tests::a_failed_screen_reset_does_not_end_the_loop` | green |
| Ctrl-T previews a palette and persists nothing | `tui::tests::app_state::cycling_the_theme_previews_it_without_choosing_it` | green |
| Choosing a palette in the overlay persists it | `tui::tests::app_state::choosing_a_palette_in_the_overlay_saves_it` | green |
| The status line stays bounded | `tui::tests::app_state::the_status_line_stays_bounded` | green |
| Every palette clears the WCAG contrast ratio | `tui::theme::tests::every_palette_is_legible` | green |
| The light palette is readable on a light ground | `tui::theme::tests::the_daylight_palette_is_readable_on_a_light_ground` | green |
| An unchosen palette follows the terminal's appearance | `tui::theme::tests::an_unchosen_theme_follows_the_terminals_appearance` | green |
| Esc-Esc keeps a draft typed in between | `tui::tests::keymap::esc_esc_does_not_destroy_a_draft_the_user_typed_in_between` | green |
| Esc-Esc still recalls when the draft has not changed | `tui::tests::keymap::esc_esc_still_recalls_when_the_draft_has_not_changed` | green |
| Enter accepts an open completion during a run | `tui::tests::keymap::enter_accepts_the_open_completion_while_a_run_is_active` | green |
| The `OSC 11` reply is parsed, bounded, and never executed | `tui::terminal_appearance::tests::` (25 tests) | green |

Commands and results, 2026-10-22 03:40–03:45 EDT:

```
./tools/cargo.sh test --locked -p abbey --lib -- tui::tests::background \
  tui::tests::app_state tui::clipboard::tests tui::run_loop::tests \
  tui::tests::keymap tui::theme::tests
  → 52 passed; 0 failed

./tools/cargo.sh test --locked -p abbey --lib -- tui::terminal_appearance
  → 25 passed; 0 failed

./tools/cargo.sh test --locked -p abbey --lib -- tui::
  → 224 passed; 2 failed
```

The two failures in the full `tui::` run are in the concurrent session's own
in-flight files, not in the cohort's tests:

- `tui::command::tests::the_shown_command_reads_back_as_the_same_arguments` —
  a Windows path round-trip in the new `src/tui/command.rs` (`C:\Users\me`
  came back as `C:Usersme`).
- `tui::prediction_owner::tests::a_detached_worker_is_reaped_once_it_has_finished`
  — reproduced three times out of three at 03:41 EDT, so it is deterministic
  in this tree, not a flake.

At 03:56 EDT the crate no longer compiled: 7 errors, all in `src/train/**`
(`SPLIT_RULE` private in `src/train/dataset.rs:155`, `src/train/run.rs:121`,
`src/train/mod.rs:175`; `as_ref` on `SqliteMemory` at
`src/train/dataset.rs:317`; an `Option<String>`/`OsStr` bound at
`src/train/mod.rs:412`; two type mismatches at `src/train/mod.rs:502` and
`src/train/mod.rs:600`). That is a separate, concurrent workstream, so the TUI
suite above could not be re-run at the moment of writing.

## Residuals

These are open, and none of them is closed by this receipt.

1. **The startup appearance query still costs two things.**
   `src/tui/terminal_appearance.rs:18-31` states both, and neither is a
   guarantee:
   - A keystroke typed inside the query window is lost. The answer arrives on
     the terminal's input queue, and that queue cannot be put back
     (`src/tui/terminal_appearance.rs:23-24`).
   - A terminal that answers after the grace has its reply read as ordinary
     key events, so the reply bytes land in the composer
     (`src/tui/terminal_appearance.rs:25-31`). The window is 120 ms
     (`src/tui/terminal_appearance.rs:57`) and the late grace 60 ms
     (`src/tui/terminal_appearance.rs:65`).
   The same two costs are recorded in
   `docs/verification/2026-10-21-evidence-and-appearance.md:31-41`.
2. **The shutdown join is unbounded for the run worker and for the prediction
   workers.** The clipboard join is bounded, but `App::shutdown` joins the run
   worker with `run.join()` (`src/tui/app.rs:769`), which is a plain
   `thread.join()` (`src/tui/worker.rs:46-51`) with no bound, and
   `PredictionOwner::join_all` does the same for every prediction worker
   (`src/tui/prediction_owner.rs:125-139`). A worker that ignores its
   cancellation token still holds the shutdown.
3. **The TUI installs no SIGINT or SIGTERM handler.** Nothing in `src/tui/`
   registers a signal handler; the only signal handling in the tree is the
   daemon's own mask (`src/bin/abbeyd.rs:29-33`). Raw mode
   (`src/tui/terminal.rs:31`) means the terminal generates no SIGINT for the
   in-process Ctrl-C, which is handled as a key event
   (`src/tui/keymap.rs:110-121`): the first Ctrl-C clears the composer and
   arms the quit, the second quits. An **external** kill therefore leaves the
   terminal un-restored — raw mode and the alternate screen stay up.
4. **The transcript is re-rendered in full every frame.** The event loop draws
   on every iteration (`src/tui/run_loop.rs:166`), each draw calls
   `transcript.lines()` (`src/tui/render.rs:132`), and that walks every cell
   and builds every line (`src/tui/transcript.rs:229-247`). There is no
   cache. The cost is described here; it was not measured.
5. **The completion menu's Enter rule was still moving.** At the snapshot,
   Enter accepts a completion the user asked for and nothing else — a path
   typed after `@`, a command name being typed after `/`, or a suggestion the
   user moved the selection onto (`src/tui/keymap.rs:303-316`,
   `src/tui/predict.rs:88-97`) — while Tab accepts whatever the menu holds
   (`src/tui/keymap.rs:183-185`). A menu the ranker opened on ordinary prose
   is none of those, so Enter submits the draft. `src/tui/keymap.rs` and
   `src/tui/predict.rs` were edited at 03:39 and 03:38 during this session:
   re-read them before relying on this paragraph.

## What this receipt does NOT prove

- It is **not** a gate result. No `./check.sh` and no `desktop/check.sh` run
  was made for this receipt, and the TUI unit suite is not the gate: it does
  not compile the three feature cohorts, does not run the PTY fixtures, and
  does not run clippy, rustdoc, the installer, or the claims sync.
- It is **not** live vendor-service, model-quality, Windows runtime, desktop,
  daemon, or owned tool-host acceptance, and it adds no training or
  production-weight acceptance.
- It does **not** prove that a terminal or multiplexer which refuses
  `ESC[6n` behaves well at runtime. The repaint change is proven at the
  function level (`needs_repaint`, `repaint_quietly`), not against a real
  multiplexer.
- It does **not** prove the appearance query is free: residual 1 stands.
- It does **not** prove that an external kill restores the terminal: residual
  3 stands.
- It does **not** measure performance. No frame timing was taken; the
  per-frame transcript re-render is described, not benchmarked.
- It does **not** prove the cohort is finished. The tree was in flight, and
  two of the tree's own TUI tests were red at 03:40 EDT for reasons recorded
  above.
