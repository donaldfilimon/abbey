//! `abbey wdbx …` — subprocess bridge to the `abi wdbx` CLI.
//!
//! This is the Phase-3 fallback for builds without `--features wdbx`: rather
//! than reimplementing WDBX query/stats surfaces, Abbey shells out to `abi`
//! when it is on PATH. Argv construction is a pure function so it can be
//! tested without an `abi` binary present.

use crate::config::{self, AbbeyConfig};
use crate::state::AbbeyState;
use anyhow::{Result, bail};
use std::process::Command;

/// Build the argv passed to the `abi` binary for `abbey wdbx <args…>`.
///
/// `query` gains `--json` automatically so callers get machine-readable output
/// unless they already asked for a specific format.
pub fn build_argv(args: &[String]) -> Vec<String> {
    let mut argv = vec!["wdbx".to_string()];
    argv.extend(args.iter().cloned());
    let is_query = args.first().is_some_and(|a| a == "query");
    if is_query && !args.iter().any(|a| a == "--json") {
        argv.push("--json".into());
    }
    argv
}

/// Human-readable note describing the bridge's availability.
pub fn availability(cfg: &AbbeyConfig) -> String {
    config::wdbx_cli_status(cfg)
}

/// Subcommands Abbey answers itself from the in-process store rather than
/// forwarding to `abi`. They need the linked backend, not a binary on PATH.
const IN_PROCESS: [&str; 2] = ["stats", "checkpoint"];

pub fn run(state: &AbbeyState, args: &[String]) -> Result<i32> {
    let cfg = AbbeyConfig::load().unwrap_or_default();
    if args.is_empty() || args[0] == "help" || args[0] == "--help" {
        println!("{}", help_text(&cfg));
        return Ok(0);
    }

    if IN_PROCESS.contains(&args[0].as_str()) {
        return run_in_process(state, &args[0]);
    }

    run_with_bin(config::resolve_abi_bin(&cfg), args)
}

#[cfg(feature = "wdbx")]
fn run_in_process(state: &AbbeyState, sub: &str) -> Result<i32> {
    use crate::memory::WdbxMemory;
    let db = WdbxMemory::open(&WdbxMemory::path_for_state_dir(&state.state_dir))?;
    match sub {
        "stats" => println!("{}", db.stats_line()),
        "checkpoint" => println!("checkpoint epoch {}", db.checkpoint()?),
        other => bail!("unknown in-process wdbx subcommand '{other}'"),
    }
    Ok(0)
}

#[cfg(not(feature = "wdbx"))]
fn run_in_process(_state: &AbbeyState, sub: &str) -> Result<i32> {
    bail!(
        "`abbey wdbx {sub}` reads the in-process store, which this binary was \
         built without.\nRebuild with `--features wdbx`."
    )
}

/// Bridge execution with the `abi` binary already resolved, so the
/// unavailable-binary path is testable without touching PATH or process env.
fn run_with_bin(bin: Option<std::path::PathBuf>, args: &[String]) -> Result<i32> {
    let Some(bin) = bin else {
        bail!(
            "`abi` is not on PATH — the WDBX CLI bridge is unavailable.\n\
             Set `abi_bin` in {} or ABBEY_ABI_BIN, or rebuild Abbey with \
             `--features wdbx` for the in-process backend.",
            AbbeyConfig::config_path().display()
        );
    };

    let argv = build_argv(args);
    let status = Command::new(&bin).args(&argv).status()?;
    Ok(status.code().unwrap_or(1))
}

fn help_text(cfg: &AbbeyConfig) -> String {
    format!(
        "abbey wdbx — bridge to the `abi wdbx` CLI\n\n\
         usage: abbey wdbx <query|db|block|benchmark|compute|…> [args…]\n\
         \x20      abbey wdbx <stats|checkpoint>        (in-process, needs --features wdbx)\n\n\
         `query` gets --json appended automatically.\n\n\
         {}\n{}\n\n\
         In-process backend: set memory_backend = \"wdbx\" (or ABBEY_MEMORY_BACKEND=wdbx)\n\
         in a build made with `--features wdbx`.",
        availability(cfg),
        crate::memory::feature_status()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn query_gets_json_appended() {
        assert_eq!(
            build_argv(&v(&["query", "/tmp/store"])),
            v(&["wdbx", "query", "/tmp/store", "--json"])
        );
    }

    #[test]
    fn existing_json_flag_is_not_duplicated() {
        assert_eq!(
            build_argv(&v(&["query", "/tmp/store", "--json"])),
            v(&["wdbx", "query", "/tmp/store", "--json"])
        );
    }

    #[test]
    fn non_query_subcommands_are_passed_through_verbatim() {
        assert_eq!(
            build_argv(&v(&["db", "verify", "/tmp/store"])),
            v(&["wdbx", "db", "verify", "/tmp/store"])
        );
        assert_eq!(
            build_argv(&v(&["compute", "info"])),
            v(&["wdbx", "compute", "info"])
        );
    }

    #[test]
    fn missing_abi_binary_is_an_error_not_a_silent_success() {
        let err = run_with_bin(None, &v(&["query", "/tmp/store"])).unwrap_err();
        assert!(
            format!("{err:#}").contains("not on PATH"),
            "expected an availability error, got: {err:#}"
        );
    }
}
