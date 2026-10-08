//! Terminal lifecycle: alternate screen, bracketed paste, mouse, the event
//! loop, and suspend/resume for `$EDITOR` and interactive slash commands.

use super::app::{App, Suspend};
use super::terminal::TerminalGuard;
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
    guard.restore()?;
    let action = match what {
        Suspend::Editor => edit_composer(app),
        Suspend::InteractiveSlash(cmd) => {
            crate::slash_dispatch::dispatch_slash(&cmd, &app.state, &mut app.cfg)
                .map(|code| app.transcript.push_notice(format!("{cmd} → exit {code}")))
        }
    };
    // Always try re-entry, even if the editor or slash command failed.
    let resume = guard
        .enter()
        .and_then(|()| term.clear().map_err(Into::into));
    if let Err(error) = action {
        app.status = format!("suspended command: {error:#}");
        app.transcript.push_error(&app.status);
    }
    resume
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
    let mut app = App::new(state, cfg)?;
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
}
