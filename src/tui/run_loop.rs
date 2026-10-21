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
use std::io::{Stdout, stdout};
use std::time::Duration;

type Term = Terminal<CrosstermBackend<Stdout>>;

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
    let resumed = take_the_terminal_back(guard, || term.clear().map_err(Into::into));
    if let Err(error) = action {
        app.status = format!("suspended command: {error:#}");
        app.transcript.push_error(&app.status);
    }
    resumed
}

/// Take the terminal back after a suspended child.
///
/// The re-entry is the point: the child had the terminal, and the app has
/// to get it back whether the child succeeded or failed. `clear` is the
/// screen reset that follows re-entry, split out so a test can drive the
/// guard sequence without a real terminal.
fn take_the_terminal_back<C: Control>(
    guard: &mut TerminalGuard<C>,
    clear: impl FnOnce() -> Result<()>,
) -> Result<()> {
    guard.enter()?;
    clear()
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
    // Ask the terminal what it looks like *before* raw mode is entered. In
    // raw mode the reply would be delivered as keyboard input and read as a
    // keypress, so the query has to happen while the terminal is still
    // line-oriented. A terminal that does not answer leaves the dark
    // palette; the wait is bounded, so an unsupported terminal is not a
    // visible stall.
    let brightness = super::terminal_appearance::background_brightness();
    let mut app = App::new_with_brightness(state, cfg, brightness)?;
    let mut guard = TerminalGuard::native();
    guard.enter()?;
    let mut term = Terminal::new(CrosstermBackend::new(stdout()))?;
    let result = (|| -> Result<i32> {
        let mut was_running = false;
        loop {
            app.pump();
            if was_running && !app.is_running() {
                term.clear()?;
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
                match event::read()? {
                    Event::Key(k) => app.handle_key(k),
                    Event::Paste(s) => app.handle_paste(&s),
                    Event::Mouse(m) => app.handle_mouse(m.kind),
                    _ => {}
                }
            }
            app.tick = app.tick.wrapping_add(1);
        }
    })();
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
        take_the_terminal_back(&mut guard, || Ok(())).unwrap();

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

        let outcome =
            take_the_terminal_back(&mut guard, || anyhow::bail!("the screen reset failed"));
        assert!(outcome.is_err(), "the reset failure must be reported");

        let recorded = calls.lock().unwrap().clone();
        for mode in [Mode::Raw, Mode::Screen, Mode::Mouse, Mode::Paste] {
            assert!(
                recorded.contains(&(mode, true)),
                "{mode:?} must be re-entered even when the reset failed"
            );
        }
    }
}
