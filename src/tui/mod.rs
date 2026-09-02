//! Abbey custom TUI (ratatui + crossterm).

mod app;
mod keys;
mod overlay;
mod predict;
mod refresh;
mod tabs;
mod theme;
mod ui;
mod widgets;

pub use app::run_tui;
