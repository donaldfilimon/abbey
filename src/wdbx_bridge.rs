//! `abbey wdbx …` — subprocess bridge to the `abi wdbx` CLI.
//!
//! This is the Phase-3 fallback for builds without `--features wdbx`: rather
//! than reimplementing WDBX query/stats surfaces, Abbey shells out to `abi`
//! when it is on PATH. Argv construction is a pure function so it can be
//! tested without an `abi` binary present.

use crate::config::{self, AbbeyConfig};
use crate::state::AbbeyState;
use anyhow::{Result, bail};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Abbey's own WDBX store as an `abi`-style **base path**.
///
/// The two tools disagree about what a "path" is: Abbey opens a *directory*
/// (`StorePaths::new` picks the `wdbx` base name inside it), while `abi wdbx`
/// splits its argument into parent-dir + base name. So Abbey's store at
/// `<state>/wdbx/` is `<state>/wdbx/wdbx` to `abi` — passing the bare directory
/// silently reads one level up and reports an empty store.
pub fn abbey_store_base(state_dir: &Path) -> PathBuf {
    state_dir.join("wdbx").join("wdbx")
}

/// Build the argv passed to the `abi` binary for `abbey wdbx <args…>`.
///
/// `query` gains `--json` automatically, and — when no path was given — Abbey's
/// own store base path, so `abbey wdbx query` just works.
pub fn build_argv(args: &[String], default_base: &Path) -> Vec<String> {
    let mut argv = vec!["wdbx".to_string()];
    argv.extend(args.iter().cloned());

    if args.first().is_some_and(|a| a == "query") {
        // A bare `query` (no store path) targets Abbey's own store.
        if !has_positional(&args[1..]) {
            argv.insert(2, default_base.display().to_string());
        }
        if !args.iter().any(|a| a == "--json") {
            argv.push("--json".into());
        }
    }
    argv
}

/// `abi wdbx query` flags that consume the following token as their value.
/// Without this, `--limit 5` would make `5` look like the store path.
const VALUE_FLAGS: [&str; 3] = ["--limit", "--text", "--persona"];

/// Whether `args` contains a positional argument — for `query`, the first one
/// is always the store path.
fn has_positional(args: &[String]) -> bool {
    let mut skip_value = false;
    for a in args {
        if skip_value {
            skip_value = false;
            continue;
        }
        if VALUE_FLAGS.contains(&a.as_str()) {
            skip_value = true;
            continue;
        }
        // Covers --json and the `--limit=5` spelling alike.
        if a.starts_with('-') {
            continue;
        }
        return true;
    }
    false
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

    let base = abbey_store_base(&state.state_dir);

    // `abi` knows nothing about Abbey's flock, so a passthrough aimed at Abbey's
    // own store would be exactly the unguarded concurrent access the lock exists
    // to prevent. Hold it for the subprocess's lifetime. (Without the `wdbx`
    // feature Abbey has no store of its own, so there is nothing to coordinate.)
    #[cfg(feature = "wdbx")]
    let _own_store_guard = if targets_abbey_store(args, &base) {
        Some(crate::memory::lock_store_dir(
            &state.state_dir.join("wdbx"),
            std::time::Duration::from_secs(10),
        )?)
    } else {
        None
    };

    run_with_bin(config::resolve_abi_bin(&cfg), args, &base)
}

/// Whether this passthrough would read or mutate Abbey's own WDBX store.
///
/// Over-locking is harmless; under-locking is the risk, so a bare `query` (which
/// gets the default injected) and any exact reference to Abbey's base path or its
/// directory all count.
#[cfg(feature = "wdbx")]
fn targets_abbey_store(args: &[String], base: &Path) -> bool {
    if args.first().is_some_and(|a| a == "query") && !has_positional(&args[1..]) {
        return true;
    }
    let base_str = base.display().to_string();
    let dir_str = base.parent().map(|p| p.display().to_string());
    args.iter()
        .any(|a| *a == base_str || Some(a.as_str()) == dir_str.as_deref())
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
fn run_with_bin(bin: Option<PathBuf>, args: &[String], default_base: &Path) -> Result<i32> {
    let Some(bin) = bin else {
        bail!(
            "`abi` is not on PATH — the WDBX CLI bridge is unavailable.\n\
             Set `abi_bin` in {} or ABBEY_ABI_BIN, or rebuild Abbey with \
             `--features wdbx` for the in-process backend.",
            AbbeyConfig::config_path().display()
        );
    };

    let argv = build_argv(args, default_base);
    let status = Command::new(&bin).args(&argv).status()?;
    Ok(status.code().unwrap_or(1))
}

fn help_text(cfg: &AbbeyConfig) -> String {
    format!(
        "abbey wdbx — bridge to the `abi wdbx` CLI\n\n\
         usage: abbey wdbx <query|db|block|benchmark|compute|…> [args…]\n\
         \x20      abbey wdbx <stats|checkpoint>        (in-process, needs --features wdbx)\n\n\
         `query` gets --json appended, and defaults to Abbey's own store.\n\
         NOTE: `abi` paths are BASE paths (dir + base name), so Abbey's store\n\
         directory `<state>/wdbx/` is `<state>/wdbx/wdbx` to `abi` — passing the\n\
         bare directory reads one level up and reports an empty store.\n\n\
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

    fn base() -> PathBuf {
        PathBuf::from("/state/wdbx/wdbx")
    }

    #[test]
    fn query_gets_json_appended() {
        assert_eq!(
            build_argv(&v(&["query", "/tmp/store"]), &base()),
            v(&["wdbx", "query", "/tmp/store", "--json"])
        );
    }

    #[test]
    fn existing_json_flag_is_not_duplicated() {
        assert_eq!(
            build_argv(&v(&["query", "/tmp/store", "--json"]), &base()),
            v(&["wdbx", "query", "/tmp/store", "--json"])
        );
    }

    #[test]
    fn bare_query_targets_abbeys_own_store() {
        assert_eq!(
            build_argv(&v(&["query"]), &base()),
            v(&["wdbx", "query", "/state/wdbx/wdbx", "--json"])
        );
    }

    #[test]
    fn bare_query_with_only_flags_still_gets_the_default_store() {
        assert_eq!(
            build_argv(&v(&["query", "--limit", "5"]), &base()),
            v(&[
                "wdbx",
                "query",
                "/state/wdbx/wdbx",
                "--limit",
                "5",
                "--json"
            ])
        );
    }

    #[test]
    fn an_explicit_path_is_never_overridden() {
        for form in [
            v(&["query", "/elsewhere/store", "--limit", "5"]),
            v(&["query", "--limit", "5", "/elsewhere/store"]),
            v(&["query", "--limit=5", "/elsewhere/store"]),
        ] {
            let argv = build_argv(&form, &base());
            assert!(
                !argv.iter().any(|a| a.contains("/state/wdbx")),
                "default injected despite explicit path: {argv:?}"
            );
            assert!(argv.contains(&"/elsewhere/store".to_string()));
        }
    }

    #[test]
    fn a_flag_value_is_not_mistaken_for_the_store_path() {
        // `5` is --limit's value, not a path, so the default must still appear.
        assert!(has_positional(&v(&["--limit", "5"])).eq(&false));
        assert!(has_positional(&v(&["--limit=5", "--json"])).eq(&false));
        assert!(has_positional(&v(&["--limit", "5", "/p"])));
    }

    #[test]
    fn non_query_subcommands_are_passed_through_verbatim() {
        assert_eq!(
            build_argv(&v(&["db", "verify", "/tmp/store"]), &base()),
            v(&["wdbx", "db", "verify", "/tmp/store"])
        );
        assert_eq!(
            build_argv(&v(&["compute", "info"]), &base()),
            v(&["wdbx", "compute", "info"])
        );
    }

    #[test]
    fn missing_abi_binary_is_an_error_not_a_silent_success() {
        let err = run_with_bin(None, &v(&["query", "/tmp/store"]), &base()).unwrap_err();
        assert!(
            format!("{err:#}").contains("not on PATH"),
            "expected an availability error, got: {err:#}"
        );
    }
}
