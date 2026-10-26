//! Abbey application library.
//!
//! The CLI, TUI, daemon, and future desktop clients share one implementation
//! crate. Only the presentation-neutral application contract and the
//! read-only effective-model summary are public; the mature CLI modules
//! remain implementation details.

pub mod abbey_contracts;
mod accel;
mod actions;
mod agent;
pub mod app_core;
mod argv_budget;
mod build_info;
mod capture;
mod claims;
mod cli;
mod commands;
mod config;
mod config_snapshot;
pub mod daemon;
mod deferred;
mod distill;
mod doctor;
pub mod edition;
pub mod effective_model;
mod entry;
mod generate;
mod gitops;
mod highlight;
mod host;
mod hybrid_loop;
mod improve;
mod init;
mod inventory;
mod knowledge;
mod learn;
mod mcp_host;
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
pub mod run_control;
pub mod runtime;
mod session;
mod slash;
mod slash_alias;
mod slash_dispatch;
mod state;
mod stream;
mod subagents;
mod surfaces;
pub mod train;
mod tui;
mod voice;
mod voice_portable;
mod wdbx_bridge;

pub use entry::run_cli;

#[cfg(all(test, unix))]
mod owned_cancel_tests;

pub mod automation_schedule;
