//! Abbey application library.
//!
//! The CLI, TUI, daemon, and future desktop clients share one implementation
//! crate. Only the presentation-neutral application contract is public; the
//! mature CLI modules remain implementation details.

mod actions;
mod agent;
pub mod app_core;
mod build_info;
mod capture;
mod claims;
mod cli;
mod commands;
mod config;
pub mod daemon;
mod deferred;
mod doctor;
mod entry;
mod generate;
mod gitops;
mod highlight;
mod host;
mod hybrid_loop;
mod improve;
mod init;
mod inventory;
mod learn;
mod media;
mod memory;
mod mesh;
mod models;
mod os_control;
mod output;
mod parallel;
mod persona;
mod platform;
mod please_fix;
mod prompts;
mod protocols;
mod roles;
mod route_log;
pub mod runtime;
mod session;
mod slash;
mod slash_dispatch;
mod state;
mod subagents;
mod surfaces;
mod tui;
mod voice;
mod wdbx_bridge;

pub use entry::run_cli;
