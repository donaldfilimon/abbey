//! Personal edition only: the structured `shell.exec` contract.
//!
//! This module is contract validation; nothing here executes. It turns an
//! untrusted JSON call into a bounded, argv-only [`contract::ShellExec`] or a
//! specific [`contract::ShellRefusal`]. There is no spawn, no daemon, MCP or
//! CLI wiring, and no claim change: `Edition::unrestricted_runtime_implemented`
//! stays `false`, and `os_control`'s allowlist and confirmation are untouched.

// The validated contract lands before its executor (authority plan Task 20);
// until then only this module's own tests use it.
#![cfg_attr(not(test), allow(dead_code))]

pub(crate) mod contract;
