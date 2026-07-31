//! Abbey custom TUI (ratatui + crossterm).

mod app;
mod overlay;
mod refresh;
mod tabs;
mod theme;
mod ui;
mod widgets;

pub use app::run_tui;
