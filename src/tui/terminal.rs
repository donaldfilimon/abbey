//! Terminal modes are owned by a guard, including partial entry and unwind.

use anyhow::Result;
use crossterm::cursor::Show;
use crossterm::event::{
    DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Mode {
    Raw,
    Screen,
    Mouse,
    Paste,
    Cursor,
}

pub(super) trait Control {
    fn set(&mut self, mode: Mode, enabled: bool) -> Result<()>;
}

pub(super) struct NativeControl;
impl Control for NativeControl {
    fn set(&mut self, mode: Mode, enabled: bool) -> Result<()> {
        let mut out = std::io::stdout();
        match (mode, enabled) {
            (Mode::Raw, true) => enable_raw_mode()?,
            (Mode::Raw, false) => disable_raw_mode()?,
            (Mode::Screen, true) => execute!(out, EnterAlternateScreen)?,
            (Mode::Screen, false) => execute!(out, LeaveAlternateScreen)?,
            (Mode::Mouse, true) => execute!(out, EnableMouseCapture)?,
            (Mode::Mouse, false) => execute!(out, DisableMouseCapture)?,
            (Mode::Paste, true) => execute!(out, EnableBracketedPaste)?,
            (Mode::Paste, false) => execute!(out, DisableBracketedPaste)?,
            (Mode::Cursor, _) => execute!(out, Show)?,
        }
        Ok(())
    }
}

pub(super) struct TerminalGuard<C: Control = NativeControl> {
    control: C,
    armed: bool,
}

impl<C: Control> TerminalGuard<C> {
    fn new(control: C) -> Self {
        Self {
            control,
            armed: false,
        }
    }

    pub(super) fn enter(&mut self) -> Result<()> {
        // Arm before the first side effect: even a failed mode command may
        // have partially changed the terminal.
        self.armed = true;
        for mode in [Mode::Raw, Mode::Screen, Mode::Mouse, Mode::Paste] {
            self.control.set(mode, true)?;
        }
        Ok(())
    }

    pub(super) fn restore(&mut self) -> Result<()> {
        if !self.armed {
            return Ok(());
        }
        let mut first = None;
        for mode in [
            Mode::Raw,
            Mode::Mouse,
            Mode::Paste,
            Mode::Screen,
            Mode::Cursor,
        ] {
            if let Err(e) = self.control.set(mode, false)
                && first.is_none()
            {
                first = Some(e);
            }
        }
        // Retry cleanup in Drop if any step failed, without masking the
        // original error from initialization, drawing, or suspension.
        self.armed = first.is_some();
        first.map_or(Ok(()), Err)
    }
}

impl TerminalGuard<NativeControl> {
    pub(super) fn native() -> Self {
        Self::new(NativeControl)
    }
}

impl<C: Control> Drop for TerminalGuard<C> {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    struct Fake {
        calls: Arc<Mutex<Vec<(Mode, bool)>>>,
        fail: Option<(Mode, bool)>,
    }
    impl Control for Fake {
        fn set(&mut self, mode: Mode, enabled: bool) -> Result<()> {
            self.calls.lock().unwrap().push((mode, enabled));
            if self.fail == Some((mode, enabled)) {
                anyhow::bail!("injected terminal error");
            }
            Ok(())
        }
    }

    #[test]
    fn partial_entry_restores_every_mode() {
        for failed in [Mode::Raw, Mode::Screen, Mode::Mouse, Mode::Paste] {
            let calls = Arc::new(Mutex::new(Vec::new()));
            {
                let mut guard = TerminalGuard::new(Fake {
                    calls: calls.clone(),
                    fail: Some((failed, true)),
                });
                assert!(guard.enter().is_err());
            }
            let calls = calls.lock().unwrap();
            for mode in [
                Mode::Raw,
                Mode::Mouse,
                Mode::Paste,
                Mode::Screen,
                Mode::Cursor,
            ] {
                assert!(calls.contains(&(mode, false)));
            }
        }
    }

    #[test]
    fn cleanup_continues_after_failure_and_drop_retries() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        {
            let mut guard = TerminalGuard::new(Fake {
                calls: calls.clone(),
                fail: Some((Mode::Raw, false)),
            });
            guard.enter().unwrap();
            assert!(guard.restore().is_err());
        }
        assert_eq!(
            calls
                .lock()
                .unwrap()
                .iter()
                .filter(|v| **v == (Mode::Cursor, false))
                .count(),
            2
        );
    }

    #[test]
    fn unwind_restores_and_successful_suspend_can_reenter() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let _ = std::panic::catch_unwind({
            let calls = calls.clone();
            move || {
                let mut guard = TerminalGuard::new(Fake { calls, fail: None });
                guard.enter().unwrap();
                guard.restore().unwrap();
                guard.enter().unwrap();
                panic!("fixture panic");
            }
        });
        assert_eq!(
            calls
                .lock()
                .unwrap()
                .iter()
                .filter(|v| **v == (Mode::Raw, false))
                .count(),
            2
        );
    }
}
