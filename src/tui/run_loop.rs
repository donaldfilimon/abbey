//! Terminal lifecycle: alternate screen, bracketed paste, mouse, the event
//! loop, and suspend/resume for `$EDITOR` and interactive slash commands.

use super::app::{App, Suspend};
use super::terminal::{Control, TerminalGuard};
use crate::agent::AgentConfig;
use crate::state::AbbeyState;
use anyhow::Result;
use crossterm::event::{self, Event};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::Rect;
use std::io::{Stdout, stdout};
use std::time::Duration;

type Term = Terminal<CrosstermBackend<Stdout>>;

/// Force a full repaint without asking the terminal where the cursor is.
///
/// `Terminal::clear` asks the terminal to report the cursor position
/// (`ESC[6n`) and waits up to two seconds for the answer. A terminal or
/// multiplexer that does not answer makes the wait time out, and the
/// error used to end the event loop. Resizing the viewport to the
/// current size clears the screen and resets the diff buffers, so the
/// next draw repaints everything, with no round trip.
fn repaint(term: &mut Term) -> Result<()> {
    let size = term.size()?;
    term.resize(Rect::new(0, 0, size.width, size.height))?;
    Ok(())
}

/// Whether the screen must be repainted after a pump.
///
/// The screen is only reset when a run has just ended: the transcript
/// grew while the run was streaming, so the last frame's cells are not
/// the new frame's cells.
fn needs_repaint(was_running: bool, is_running: bool) -> bool {
    was_running && !is_running
}

/// Repaint the screen, containing a failure.
///
/// A failed screen reset must not end the event loop. The reset is a
/// convenience, not a correctness requirement: the next draw paints the
/// whole viewport anyway, and a terminal that cannot be reset is still a
/// terminal the app can use.
fn repaint_quietly(app: &mut App, repaint: impl FnOnce() -> Result<()>) {
    if let Err(error) = repaint() {
        app.status = format!("screen refresh failed: {error:#}");
    }
}

fn suspend(term: &mut Term, guard: &mut TerminalGuard, app: &mut App, what: Suspend) -> Result<()> {
    suspend_with(term, guard, app, what)
}

/// The suspend/resume sequence, over any guard.
///
/// Split out so a test can drive the real sequence with a recording
/// control: the point of the sequence is that the terminal is handed to
/// the child and then *taken back*, and that the taking-back happens even
/// when the child failed. A test that only exercised `TerminalGuard` in
/// isolation would not notice if the resume were dropped.
fn suspend_with<C: Control>(
    term: &mut Term,
    guard: &mut TerminalGuard<C>,
    app: &mut App,
    what: Suspend,
) -> Result<()> {
    guard.restore()?;
    let action = match what {
        Suspend::Editor => edit_composer(app),
        Suspend::InteractiveSlash(cmd) => {
            crate::slash_dispatch::dispatch_slash(&cmd, &app.state, &mut app.cfg)
                .map(|code| app.transcript.push_notice(format!("{cmd} → exit {code}")))
        }
    };
    // Always try re-entry, even if the editor or slash command failed.
    // The screen reset after a child uses the same repaint as the rest of
    // the loop: asking the terminal where the cursor is can block for two
    // seconds, and its failure must not end the app.
    take_the_terminal_back(guard, app, || repaint(term))?;
    if let Err(error) = action {
        app.status = format!("suspended command: {error:#}");
        app.transcript.push_error(&app.status);
    }
    Ok(())
}

/// Take the terminal back after a suspended child.
///
/// The re-entry is the point: the child had the terminal, and the app has
/// to get it back whether the child succeeded or failed. `clear` is the
/// screen reset that follows re-entry, split out so a test can drive the
/// guard sequence without a real terminal.
fn take_the_terminal_back<C: Control>(
    guard: &mut TerminalGuard<C>,
    app: &mut App,
    clear: impl FnOnce() -> Result<()>,
) -> Result<()> {
    guard.enter()?;
    repaint_quietly(app, clear);
    Ok(())
}

fn edit_composer(app: &mut App) -> Result<()> {
    let path = app.state.state_dir.join("tui-compose.md");
    std::fs::write(&path, &app.composer.text)?;
    let editor = std::env::var("VISUAL")
        .or_else(|_| std::env::var("EDITOR"))
        .unwrap_or_else(|_| "vi".into());
    edit_with(app, &path, &editor)
}

fn edit_with(app: &mut App, path: &std::path::Path, editor: &str) -> Result<()> {
    let mut parts = editor.split_whitespace();
    let bin = parts.next().unwrap_or("vi");
    let status = std::process::Command::new(bin)
        .args(parts)
        .arg(path)
        .status()?;
    anyhow::ensure!(status.success(), "editor exited {status}");
    // Editor content is a draft, including its trailing whitespace.
    app.composer.set(std::fs::read_to_string(path)?);
    app.update_completion();
    Ok(())
}

pub fn run_tui(state: AbbeyState, cfg: AgentConfig) -> Result<i32> {
    // The handlers go in before the terminal does. A signal that arrived
    // between the two would take the default disposition: the process
    // would die where it stood, the guard's `Drop` would never run, and
    // the user would be left with echo off, raw mode, and the alternate
    // screen still up.
    if let Err(error) = super::terminal::install_termination_handlers() {
        eprintln!("abbey: a termination signal will not restore the terminal ({error:#})");
    }
    let mut guard = TerminalGuard::native();
    guard.enter()?;
    // Ask the terminal what it looks like, *after* the alternate screen is
    // entered and before the first draw.
    //
    // The order matters twice over. After the alternate screen: a terminal
    // that does not understand the query may render its characters, and
    // the alternate screen is cleared, so nothing is left on the display.
    // Before the first draw: the reply is read here, and a reply that
    // arrives later would otherwise be read as a keypress.
    //
    // In raw mode the terminal does not echo the query, so the bytes are
    // not read back as input. A terminal that does not answer leaves the
    // dark palette; the wait is bounded, so an unsupported terminal is not
    // a visible stall.
    let appearance = super::terminal_appearance::appearance();
    let mut app = App::new_with_brightness(state, cfg, appearance.brightness)?;
    // The question was asked on the terminal's input stream, so it took
    // the user's keystrokes with it. They are given back here, through the
    // same path the event loop uses, before the first draw: a keystroke is
    // never spent on learning a colour.
    for key in super::terminal::decode_keystrokes(&appearance.keystrokes) {
        app.handle_key(key);
    }
    // The panels are filled by a background refresh: the doctor panel's
    // `--version` probe and the memory backend's open are slow, and they
    // must not run on the thread that draws.
    app.request_refresh();
    let mut term = Terminal::new(CrosstermBackend::new(stdout()))?;
    let result = (|| -> Result<i32> {
        let mut was_running = false;
        loop {
            // A termination signal ends the session the ordinary way: the
            // loop leaves, the guard restores the terminal, and the user is
            // not left with a broken terminal and a `reset` to type.
            if super::terminal::termination_requested() {
                app.should_quit = true;
            }
            app.pump();
            if needs_repaint(was_running, app.is_running()) {
                // A screen reset that fails is reported, never fatal: the
                // event loop has to survive a terminal that will not
                // answer.
                repaint_quietly(&mut app, || repaint(&mut term));
            }
            was_running = app.is_running();
            term.draw(|f| super::render::draw(f, &app))?;
            if let Some(what) = app.pending_suspend.take() {
                suspend(&mut term, &mut guard, &mut app, what)?;
            }
            if app.should_quit {
                return Ok(0);
            }
            let wait = if app.is_running() { 33 } else { 100 };
            if event::poll(Duration::from_millis(wait))? {
                // The signal may have arrived while the loop waited, so the
                // flag is read again before the event: the app leaves at
                // once rather than after the next keypress.
                if super::terminal::termination_requested() {
                    app.should_quit = true;
                } else {
                    match event::read()? {
                        Event::Key(k) => app.handle_key(k),
                        Event::Paste(s) => app.handle_paste(&s),
                        Event::Mouse(m) => app.handle_mouse(m.kind),
                        _ => {}
                    }
                }
            }
            app.tick = app.tick.wrapping_add(1);
        }
    })();
    // The terminal is restored whatever the loop did. `TerminalGuard` has
    // a `Drop` that restores every mode as well, so a panic in the loop
    // cannot leave the terminal in raw mode either.
    let shutdown = app.shutdown();
    let cleanup = guard.restore();
    // Preserve the loop's error over teardown or terminal cleanup errors.
    result.and_then(|code| shutdown.and(cleanup).map(|()| code))
}

#[cfg(test)]
mod tests {
    use super::super::terminal::Mode;
    use super::*;
    #[test]
    fn failed_editor_preserves_the_draft() {
        let mut app =
            super::super::tests::scratch_app("editor-fail", crate::agent::AgentBackend::Ollama);
        app.composer.set("keep draft  \n".into());
        let path = app.state.state_dir.join("edit.md");
        std::fs::write(&path, "replacement").unwrap();
        assert!(edit_with(&mut app, &path, "/nonexistent/abbey-fixture-editor").is_err());
        assert_eq!(app.composer.text, "keep draft  \n");
    }

    // Append inside existing src/tui/run_loop.rs #[cfg(test)] mod tests.
    // External current-API draft, uncompiled/unexecuted by this agent.
    // /usr/bin/true is an owned invocation of a fixed local no-op, not an editor
    // or provider selected from ambient PATH; the input file is owned scratch.

    #[cfg(unix)]
    #[test]
    fn successful_editor_return_invalidates_a_prior_file_range() {
        let mut app = super::super::tests::scratch_app(
            "editor-completion",
            crate::agent::AgentBackend::Cursor,
        );
        app.files = super::super::completion::FileIndex::from_paths(vec!["apple.rs".into()]);
        app.handle_paste("look @app");
        assert!(matches!(
            &app.completion,
            Some(super::super::completion::Completion::File { token_start: 5, .. })
        ));
        let path = app.state.state_dir.join("edit-completion.md");
        std::fs::write(&path, "other @app").unwrap();
        edit_with(&mut app, &path, "/usr/bin/true").unwrap();
        assert_eq!(app.composer.text, "other @app");
        assert_eq!(app.composer.cursor, "other @app".len());
        assert!(
            app.completion.is_none()
                || matches!(
                    &app.completion,
                    Some(super::super::completion::Completion::File { token_start: 6, .. })
                ),
            "editor retained a byte range from the previous draft"
        );
        app.handle_key(crossterm::event::KeyEvent {
            code: crossterm::event::KeyCode::Tab,
            modifiers: crossterm::event::KeyModifiers::NONE,
            kind: crossterm::event::KeyEventKind::Press,
            state: crossterm::event::KeyEventState::NONE,
        });
        assert!(app.composer.text == "other @app" || app.composer.text == "other @apple.rs ");
        assert!(app.run.is_none());
    }

    #[test]
    fn review_suspend_propagates_terminal_reentry_failure() {
        struct RefuseEntry;
        impl Control for RefuseEntry {
            fn set(&mut self, _mode: Mode, enabled: bool) -> Result<()> {
                if enabled {
                    anyhow::bail!("fixture reentry refused");
                }
                Ok(())
            }
        }
        let mut app =
            super::super::tests::scratch_app("review-reentry", crate::agent::AgentBackend::Ollama);
        let mut term = Terminal::with_options(
            CrosstermBackend::new(stdout()),
            ratatui::TerminalOptions {
                viewport: ratatui::Viewport::Fixed(Rect::new(0, 0, 10, 4)),
            },
        )
        .unwrap();
        let mut guard = TerminalGuard::new(RefuseEntry);
        let result = suspend_with(
            &mut term,
            &mut guard,
            &mut app,
            Suspend::InteractiveSlash("/no-such-review-command".into()),
        );
        assert!(
            result.is_err(),
            "terminal entry failure must reach the event loop"
        );
    }

    /// A control that records every mode change it is asked for.
    struct Recorder {
        calls: std::sync::Arc<std::sync::Mutex<Vec<(Mode, bool)>>>,
    }
    impl Control for Recorder {
        fn set(&mut self, mode: Mode, enabled: bool) -> Result<()> {
            self.calls.lock().unwrap().push((mode, enabled));
            Ok(())
        }
    }

    /// The terminal is handed to the child and then taken back.
    ///
    /// The guard's own tests cover `TerminalGuard` in isolation. This covers
    /// the *sequence* the suspend path uses: restore every mode so the child
    /// has a normal terminal, then re-enter every mode so the app has its
    /// terminal back. A resume that was dropped would leave the app drawing
    /// into a terminal the child had, and nothing in the guard's own tests
    /// would notice.
    #[test]
    fn the_resume_takes_the_terminal_back_from_the_child() {
        let calls = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut guard = TerminalGuard::new(Recorder {
            calls: calls.clone(),
        });
        guard.enter().unwrap();

        // The child gets a normal terminal: every mode is off.
        guard.restore().unwrap();
        // The app takes it back: every mode is on again.
        take_the_terminal_back(
            &mut guard,
            &mut super::super::tests::scratch_app("resume", crate::agent::AgentBackend::Ollama),
            || Ok(()),
        )
        .unwrap();

        let recorded = calls.lock().unwrap().clone();
        let entered = |mode: Mode| recorded.contains(&(mode, true));
        let exited = |mode: Mode| recorded.contains(&(mode, false));
        for mode in [Mode::Raw, Mode::Screen, Mode::Mouse, Mode::Paste] {
            assert!(entered(mode), "{mode:?} must be re-entered after the child");
            assert!(exited(mode), "{mode:?} must be released to the child");
        }
    }

    /// A failing screen reset does not cost the app its terminal.
    ///
    /// The re-entry happens before the reset, so a reset that fails leaves
    /// the terminal in the app's hands and only the redraw is missing.
    #[test]
    fn a_failing_reset_after_re_entry_still_leaves_the_terminal_in_the_apps_hands() {
        let calls = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut guard = TerminalGuard::new(Recorder {
            calls: calls.clone(),
        });
        guard.enter().unwrap();
        guard.restore().unwrap();

        let mut app =
            super::super::tests::scratch_app("repaint", crate::agent::AgentBackend::Ollama);
        let outcome = take_the_terminal_back(&mut guard, &mut app, || {
            anyhow::bail!("the screen reset failed")
        });
        assert!(outcome.is_ok(), "repaint failure must remain nonfatal");
        assert!(app.status.contains("the screen reset failed"));

        let recorded = calls.lock().unwrap().clone();
        for mode in [Mode::Raw, Mode::Screen, Mode::Mouse, Mode::Paste] {
            assert!(
                recorded.contains(&(mode, true)),
                "{mode:?} must be re-entered even when the reset failed"
            );
        }
    }

    /// The screen is only reset when a run has just ended.
    ///
    /// The reset used to be `Terminal::clear`, which asks the terminal
    /// where the cursor is and waits up to two seconds for an answer that
    /// a multiplexer may never give.
    #[test]
    fn the_screen_is_reset_only_when_a_run_has_just_ended() {
        assert!(needs_repaint(true, false));
        assert!(!needs_repaint(false, false));
        assert!(!needs_repaint(true, true));
        assert!(!needs_repaint(false, true));
    }

    /// A screen reset that fails does not end the event loop.
    ///
    /// The reset is a convenience: the next draw paints the whole
    /// viewport anyway. A terminal that cannot be reset is still a
    /// terminal the app can use, so the failure is reported and the loop
    /// keeps running.
    #[test]
    fn a_failed_screen_reset_does_not_end_the_loop() {
        let mut app =
            super::super::tests::scratch_app("repaint-failure", crate::agent::AgentBackend::Ollama);
        repaint_quietly(&mut app, || anyhow::bail!("the terminal did not answer"));

        assert!(
            app.status.contains("screen refresh failed"),
            "the failure must be reported, said {:?}",
            app.status
        );
        assert!(!app.should_quit, "a failed reset must not quit the app");
        // The app is still usable: the loop carries on with it.
        app.handle_key(crossterm::event::KeyEvent {
            code: crossterm::event::KeyCode::Char('x'),
            modifiers: crossterm::event::KeyModifiers::NONE,
            kind: crossterm::event::KeyEventKind::Press,
            state: crossterm::event::KeyEventState::NONE,
        });
        assert_eq!(app.composer.text, "x");
    }

    /// A clipboard worker that never answers does not hold the shutdown.
    ///
    /// The worker exists so a hung clipboard command cannot freeze the
    /// event loop. A shutdown that waited for it for ever would put that
    /// freeze back, with the terminal still in raw mode.
    #[test]
    fn a_clipboard_worker_that_never_answers_does_not_block_the_shutdown() {
        let mut app = super::super::tests::scratch_app(
            "clipboard-shutdown",
            crate::agent::AgentBackend::Ollama,
        );
        app.clipboard = Some(super::super::clipboard::PendingCopy::start_with(|| {
            std::thread::sleep(std::time::Duration::from_secs(30));
            super::super::clipboard::CopyOutcome::Copied
        }));

        let started = std::time::Instant::now();
        app.shutdown().unwrap();
        assert!(
            started.elapsed() < std::time::Duration::from_secs(2),
            "the shutdown must not wait for a clipboard worker that never answers"
        );
    }
}
