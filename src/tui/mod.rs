//! Abbey chat-first TUI (ratatui + crossterm).

mod app;
mod completion;
mod composer;
mod keymap;
pub(crate) mod markdown;
mod overlays;
mod permission;
mod predict;
mod prediction_owner;
mod refresh;
mod render;
mod run_loop;
mod terminal;
mod theme;
mod transcript;
mod widgets;
mod worker;

#[cfg(test)]
mod tests;

pub use run_loop::run_tui;

pub(crate) mod local_recipe;
