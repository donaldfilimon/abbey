//! Argument parsing for `abbey train`.
//!
//! The verb surface is small and closed: `plan`, `prepare`, `run`,
//! `evaluate`, `rollback`, `status`, and `help`. Anything else is a usage
//! error.

use crate::train::dataset::Source;
use anyhow::{Result, bail};
use std::path::PathBuf;

/// What `abbey train` was asked to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verb {
    /// Read-only preview. Writes nothing.
    Plan,
    /// Write a dataset and a manifest.
    Prepare,
    /// Train, evaluate, and decide.
    Run,
    /// Re-score the current artifact. Writes nothing.
    Evaluate,
    /// Restore a previous artifact.
    Rollback,
    /// Summarise the ledger and the current artifact.
    Status,
    /// Print the usage.
    Help,
}

/// The parsed options of one `abbey train` invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Options {
    /// The verb.
    pub verb: Verb,
    /// The corpus source.
    pub source: Source,
    /// The consent file, when one was named.
    pub consent: Option<PathBuf>,
    /// The run to restore, for `rollback --to`.
    pub to: Option<String>,
}

/// Parse the arguments that follow `abbey train`.
pub fn parse(args: &[String]) -> Result<Options> {
    let Some(first) = args.first() else {
        bail!("abbey train needs a verb");
    };
    if first == "--help" || first == "-h" || first == "help" {
        return Ok(Options {
            verb: Verb::Help,
            source: Source::Memory,
            consent: None,
            to: None,
        });
    }
    let verb = match first.as_str() {
        "plan" => Verb::Plan,
        "prepare" => Verb::Prepare,
        "run" => Verb::Run,
        "evaluate" | "eval" => Verb::Evaluate,
        "rollback" | "revert" => Verb::Rollback,
        "status" | "show" => Verb::Status,
        other => bail!("unknown train verb `{other}`"),
    };
    let mut source = Source::Memory;
    let mut consent: Option<PathBuf> = None;
    let mut to: Option<String> = None;
    let mut index = 1usize;
    while index < args.len() {
        let arg = args[index].as_str();
        if arg == "--help" || arg == "-h" {
            return Ok(Options {
                verb: Verb::Help,
                source,
                consent,
                to,
            });
        }
        let (name, inline) = match arg.split_once('=') {
            Some((name, value)) => (name, Some(value.to_string())),
            None => (arg, None),
        };
        let take_value = |index: &mut usize| -> Result<String> {
            match inline.clone() {
                Some(value) => Ok(value),
                None => {
                    *index += 1;
                    args.get(*index)
                        .cloned()
                        .ok_or_else(|| anyhow::anyhow!("{name} needs a value"))
                }
            }
        };
        match name {
            "--source" => {
                let value = take_value(&mut index)?;
                source = Source::parse(&value)
                    .ok_or_else(|| anyhow::anyhow!("unknown source `{value}` (memory|specs)"))?;
            }
            "--consent" => {
                let value = take_value(&mut index)?;
                consent = Some(PathBuf::from(value));
            }
            "--to" => {
                let value = take_value(&mut index)?;
                to = Some(value);
            }
            other => bail!("unknown train option `{other}`"),
        }
        index += 1;
    }
    Ok(Options {
        verb,
        source,
        consent,
        to,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn the_verbs_are_exact() {
        assert_eq!(parse(&args(&["plan"])).unwrap().verb, Verb::Plan);
        assert_eq!(parse(&args(&["prepare"])).unwrap().verb, Verb::Prepare);
        assert_eq!(parse(&args(&["run"])).unwrap().verb, Verb::Run);
        assert_eq!(parse(&args(&["evaluate"])).unwrap().verb, Verb::Evaluate);
        assert_eq!(parse(&args(&["rollback"])).unwrap().verb, Verb::Rollback);
        assert_eq!(parse(&args(&["status"])).unwrap().verb, Verb::Status);
        assert_eq!(parse(&args(&["--help"])).unwrap().verb, Verb::Help);
        assert!(parse(&args(&["nope"])).is_err());
        assert!(parse(&args(&[])).is_err());
    }

    #[test]
    fn the_flags_are_parsed() {
        let options = parse(&args(&[
            "run",
            "--source",
            "specs",
            "--consent",
            "/tmp/consent.txt",
        ]))
        .unwrap();
        assert_eq!(options.source, Source::Specs);
        assert_eq!(options.consent, Some(PathBuf::from("/tmp/consent.txt")));
        let options = parse(&args(&["plan", "--source=specs"])).unwrap();
        assert_eq!(options.source, Source::Specs);
        let options = parse(&args(&["rollback", "--to", "run-1"])).unwrap();
        assert_eq!(options.to.as_deref(), Some("run-1"));
    }

    #[test]
    fn an_unknown_option_or_source_is_refused() {
        assert!(parse(&args(&["plan", "--nope"])).is_err());
        assert!(parse(&args(&["plan", "--source", "quesar"])).is_err());
        assert!(parse(&args(&["plan", "--source"])).is_err());
    }
}
