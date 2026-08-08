use anyhow::{Result, bail};

#[derive(Debug, Clone, Default)]
pub struct RunOptions {
    pub lanes: Vec<String>,
    pub peers: Vec<String>,
    pub jobs: usize,
    pub synthesize: bool,
    pub prompt: Vec<String>,
}

fn default_jobs() -> usize {
    crate::platform::default_subagent_jobs()
}

/// Parse CLI/slash args into [`RunOptions`].
pub fn parse_args(args: &[String]) -> Result<RunOptions> {
    let mut opts = RunOptions {
        jobs: default_jobs(),
        ..RunOptions::default()
    };
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "run" | "exec" => {}
            "list" | "ls" | "catalog" | "status" => return Ok(opts),
            "--lanes" | "-l" => {
                index += 1;
                let Some(value) = args.get(index) else {
                    bail!("--lanes needs a comma-separated list");
                };
                opts.lanes = split_csv(value);
            }
            "--peers" | "-P" => {
                index += 1;
                let Some(value) = args.get(index) else {
                    bail!("--peers needs a comma-separated list");
                };
                opts.peers = split_csv(value);
            }
            "--jobs" | "-j" => {
                opts.jobs =
                    crate::slash::parse_jobs_value(args.get(index + 1).map(String::as_str))?;
                index += 1;
            }
            "--synthesize" | "--merge" | "-s" => opts.synthesize = true,
            "-h" | "--help" => return Ok(opts),
            value if value.starts_with("--lanes=") => {
                opts.lanes = split_csv(value.trim_start_matches("--lanes="));
            }
            value if value.starts_with("--peers=") => {
                opts.peers = split_csv(value.trim_start_matches("--peers="));
            }
            value if value.starts_with("--jobs=") => {
                opts.jobs = crate::slash::parse_jobs_value(value.strip_prefix("--jobs="))?;
            }
            value if value.starts_with('-') => bail!("unknown subagents flag: {value}"),
            value => opts.prompt.push(value.to_string()),
        }
        index += 1;
    }
    Ok(opts)
}

fn split_csv(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(str::to_ascii_lowercase)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_lanes_peers_synthesize() {
        let args = vec![
            "run".into(),
            "--lanes".into(),
            "max,reviewer".into(),
            "--peers".into(),
            "gemini".into(),
            "--synthesize".into(),
            "--jobs".into(),
            "2".into(),
            "fix".into(),
            "it".into(),
        ];
        let opts = parse_args(&args).unwrap();
        assert_eq!(opts.lanes, vec!["max", "reviewer"]);
        assert_eq!(opts.peers, vec!["gemini"]);
        assert!(opts.synthesize);
        assert_eq!(opts.jobs, 2);
        assert_eq!(opts.prompt, vec!["fix", "it"]);
    }

    #[test]
    fn jobs_reject_zero_malformed_and_missing_values() {
        for args in [
            vec!["--jobs".into()],
            vec!["--jobs".into(), "0".into()],
            vec!["--jobs".into(), "many".into()],
            vec!["--jobs=0".into()],
            vec!["--jobs=many".into()],
        ] {
            assert!(parse_args(&args).is_err(), "accepted {args:?}");
        }
    }

    #[test]
    fn jobs_accept_separated_and_equals_forms() {
        assert_eq!(parse_args(&["--jobs".into(), "3".into()]).unwrap().jobs, 3);
        assert_eq!(parse_args(&["--jobs=4".into()]).unwrap().jobs, 4);
    }
}
