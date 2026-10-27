//! Validation of one structured `shell.exec` call.
//!
//! The call is argv only: an absolute program, discrete arguments, an
//! absolute working directory, an explicit environment, and bounded timeout
//! and capture sizes. A shell string, a `command`/`shell` key, or a shell
//! interpreter asked to run a command string is refused before anything
//! else. Validation performs no PATH search, no filesystem access, and no
//! process creation.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;

use crate::argv_budget::{MAX_PROCESS_ARG_BYTES, MAX_PROCESS_TOTAL_ARG_BYTES};

/// The longest a single personal `shell.exec` may run.
pub(crate) const MAX_SHELL_TIMEOUT: Duration = Duration::from_secs(10 * 60);

/// The most output bytes a single personal `shell.exec` may capture.
pub(crate) const MAX_SHELL_CAPTURE_BYTES: usize = 1024 * 1024;

/// A validated, argv-only execution request. Holding one grants nothing; it
/// only records that the call passed the contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ShellExec {
    pub(crate) program: PathBuf,
    pub(crate) args: Vec<OsString>,
    pub(crate) cwd: PathBuf,
    pub(crate) env: BTreeMap<OsString, OsString>,
    pub(crate) timeout: Duration,
    pub(crate) capture_bytes: usize,
}

/// Why a call was refused. Each variant names one contract rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ShellRefusal {
    /// A shell command string, or an object carrying `command`/`shell`.
    ShellString,
    /// A shell interpreter asked to evaluate a command string (`sh -c`).
    ShellInterpreterCommand,
    /// `program` is not an absolute path; there is no PATH search.
    ProgramNotAbsolute,
    /// `cwd` is not an absolute path.
    CwdNotAbsolute,
    /// An environment key is empty or contains `=`.
    InvalidEnvKey,
    /// A NUL byte in the program, an argument, the cwd, or the environment.
    NulByte,
    /// One argument exceeds the per-argument supervisor budget.
    ArgTooLarge,
    /// Program, arguments and environment together exceed the total budget.
    ArgvBudgetExceeded,
    /// `timeout_ms` is zero or above [`MAX_SHELL_TIMEOUT`].
    TimeoutOutOfRange,
    /// `capture_bytes` is zero or above [`MAX_SHELL_CAPTURE_BYTES`].
    CaptureOutOfRange,
    /// Not an object of exactly the contract's fields and types.
    Malformed,
}

/// The exact wire shape. Unknown fields are refused, never ignored.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Wire {
    program: String,
    args: Vec<String>,
    cwd: String,
    #[serde(default)]
    env: BTreeMap<String, String>,
    timeout_ms: u64,
    capture_bytes: u64,
}

/// Interpreter file names whose command-string flags are refused.
const POSIX_SHELLS: &[&str] = &["sh", "bash", "zsh", "dash", "ksh", "fish", "csh", "tcsh"];
const WINDOWS_SHELLS: &[&str] = &[
    "cmd",
    "cmd.exe",
    "powershell",
    "powershell.exe",
    "pwsh",
    "pwsh.exe",
];
/// Flags that make an interpreter evaluate the next argument as a command
/// string. Windows interpreters compare them ASCII case-insensitively; POSIX
/// shells compare exactly, so `bash -C` (noclobber) stays an ordinary flag.
const COMMAND_FLAGS: &[&str] = &["-c", "/c", "/C", "-Command", "-EncodedCommand"];

/// Validate one untrusted `shell.exec` call.
pub(crate) fn validate(call: &serde_json::Value) -> Result<ShellExec, ShellRefusal> {
    // A shell string is refused specifically, before generic decoding.
    match call {
        serde_json::Value::String(_) => return Err(ShellRefusal::ShellString),
        serde_json::Value::Object(map)
            if map.contains_key("command") || map.contains_key("shell") =>
        {
            return Err(ShellRefusal::ShellString);
        }
        serde_json::Value::Object(_) => {}
        _ => return Err(ShellRefusal::Malformed),
    }
    let wire = Wire::deserialize(call).map_err(|_| ShellRefusal::Malformed)?;

    let has_nul = |s: &str| s.contains('\0');
    if has_nul(&wire.program)
        || has_nul(&wire.cwd)
        || wire.args.iter().any(|a| has_nul(a))
        || wire.env.iter().any(|(k, v)| has_nul(k) || has_nul(v))
    {
        return Err(ShellRefusal::NulByte);
    }

    // No PATH search: a bare name or relative path never resolves.
    let program = PathBuf::from(&wire.program);
    if !program.is_absolute() {
        return Err(ShellRefusal::ProgramNotAbsolute);
    }
    let cwd = PathBuf::from(&wire.cwd);
    if !cwd.is_absolute() {
        return Err(ShellRefusal::CwdNotAbsolute);
    }
    if wire.env.keys().any(|k| k.is_empty() || k.contains('=')) {
        return Err(ShellRefusal::InvalidEnvKey);
    }
    if runs_a_command_string(&program, &wire.args) {
        return Err(ShellRefusal::ShellInterpreterCommand);
    }

    if wire.args.iter().any(|a| a.len() > MAX_PROCESS_ARG_BYTES) {
        return Err(ShellRefusal::ArgTooLarge);
    }
    let total = wire.program.len()
        + wire.args.iter().map(String::len).sum::<usize>()
        + wire
            .env
            .iter()
            .map(|(k, v)| k.len() + 1 + v.len())
            .sum::<usize>();
    if total > MAX_PROCESS_TOTAL_ARG_BYTES {
        return Err(ShellRefusal::ArgvBudgetExceeded);
    }

    let timeout = Duration::from_millis(wire.timeout_ms);
    if timeout.is_zero() || timeout > MAX_SHELL_TIMEOUT {
        return Err(ShellRefusal::TimeoutOutOfRange);
    }
    let capture_bytes = usize::try_from(wire.capture_bytes)
        .ok()
        .filter(|n| (1..=MAX_SHELL_CAPTURE_BYTES).contains(n))
        .ok_or(ShellRefusal::CaptureOutOfRange)?;

    Ok(ShellExec {
        program,
        args: wire.args.into_iter().map(OsString::from).collect(),
        cwd,
        env: wire
            .env
            .into_iter()
            .map(|(k, v)| (OsString::from(k), OsString::from(v)))
            .collect(),
        timeout,
        capture_bytes,
    })
}

/// Whether `program` is a shell interpreter asked to evaluate a command
/// string. POSIX shells also accept `-c` inside combined short flags
/// (`bash -lc`), which counts too.
fn runs_a_command_string(program: &Path, args: &[String]) -> bool {
    let Some(name) = program.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    let name = name.to_ascii_lowercase();
    let posix = POSIX_SHELLS.contains(&name.as_str());
    if !posix && !WINDOWS_SHELLS.contains(&name.as_str()) {
        return false;
    }
    args.iter().any(|arg| {
        COMMAND_FLAGS.iter().any(|f| {
            if posix {
                arg == f
            } else {
                arg.eq_ignore_ascii_case(f)
            }
        }) || (posix
            && arg.len() > 1
            && arg.starts_with('-')
            && !arg.starts_with("--")
            && arg[1..].bytes().all(|b| b.is_ascii_alphabetic())
            && arg[1..].contains('c'))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    /// An absolute path on this host (Windows needs a drive prefix).
    fn abs(unix: &str) -> String {
        if cfg!(windows) {
            format!("C:{}", unix.replace('/', "\\"))
        } else {
            unix.to_string()
        }
    }

    fn call(program: &str, args: &[&str]) -> Value {
        json!({
            "program": program,
            "args": args,
            "cwd": abs("/tmp"),
            "env": {},
            "timeout_ms": 1000,
            "capture_bytes": 4096,
        })
    }

    fn with(mut v: Value, key: &str, value: Value) -> Value {
        v[key] = value;
        v
    }

    #[test]
    fn a_shell_string_is_refused_and_only_argv_is_accepted() {
        assert_eq!(
            validate(&json!("ls -la /tmp")),
            Err(ShellRefusal::ShellString)
        );
        assert_eq!(
            validate(&json!({"command": "ls -la"})),
            Err(ShellRefusal::ShellString)
        );
        let ok = call(&abs("/bin/ls"), &["-la"]);
        assert_eq!(
            validate(&with(ok.clone(), "shell", json!(true))),
            Err(ShellRefusal::ShellString)
        );
        assert_eq!(
            validate(&with(ok.clone(), "command", json!("ls"))),
            Err(ShellRefusal::ShellString)
        );
        assert_eq!(validate(&json!(["ls"])), Err(ShellRefusal::Malformed));

        let ok = with(ok, "env", json!({"LANG": "C", "EMPTY": ""}));
        let exec = validate(&ok).expect("a structured argv call is accepted");
        assert_eq!(exec.program, PathBuf::from(abs("/bin/ls")));
        assert_eq!(exec.args, vec![OsString::from("-la")]);
        assert_eq!(exec.cwd, PathBuf::from(abs("/tmp")));
        let env: BTreeMap<OsString, OsString> = [("EMPTY", ""), ("LANG", "C")]
            .into_iter()
            .map(|(k, v)| (OsString::from(k), OsString::from(v)))
            .collect();
        assert_eq!(exec.env, env);
        assert_eq!(exec.timeout, Duration::from_millis(1000));
        assert_eq!(exec.capture_bytes, 4096);

        // `env` is optional and defaults to empty.
        let mut no_env = call(&abs("/bin/ls"), &[]);
        no_env.as_object_mut().unwrap().remove("env");
        assert!(validate(&no_env).expect("env defaults").env.is_empty());
    }

    #[test]
    fn sh_dash_c_is_refused_even_as_argv() {
        for shell in [
            "sh",
            "bash",
            "zsh",
            "dash",
            "ksh",
            "fish",
            "csh",
            "tcsh",
            "cmd",
            "cmd.exe",
            "powershell",
            "powershell.exe",
            "pwsh",
            "pwsh.exe",
        ] {
            let program = abs(&format!("/usr/bin/{shell}"));
            for flag in ["-c", "/c", "/C", "-Command", "-EncodedCommand"] {
                assert_eq!(
                    validate(&call(&program, &[flag, "echo hi"])),
                    Err(ShellRefusal::ShellInterpreterCommand),
                    "{shell} {flag}"
                );
            }
            // The flag anywhere in argv counts, not only first.
            assert_eq!(
                validate(&call(&program, &["-e", "-c", "echo hi"])),
                Err(ShellRefusal::ShellInterpreterCommand),
                "{shell} with -c later"
            );
        }
        // Combined POSIX short flags still carry `-c`.
        assert_eq!(
            validate(&call(&abs("/bin/bash"), &["-lc", "echo hi"])),
            Err(ShellRefusal::ShellInterpreterCommand)
        );
        // Windows interpreters ignore flag case; POSIX `-C` is noclobber.
        assert_eq!(
            validate(&call(&abs("/w/PowerShell.exe"), &["-command", "x"])),
            Err(ShellRefusal::ShellInterpreterCommand)
        );
        assert!(validate(&call(&abs("/bin/bash"), &["-C", &abs("/tmp/s.sh")])).is_ok());
        // A shell running a script file is argv, not a command string.
        assert!(validate(&call(&abs("/bin/sh"), &[&abs("/tmp/script.sh")])).is_ok());
        // `-c` to a non-shell program is an ordinary argument.
        assert!(validate(&call(&abs("/usr/bin/grep"), &["-c", "x"])).is_ok());
    }

    #[test]
    fn relative_program_paths_resolve_without_path_search_surprises() {
        for program in ["ls", "./ls", "bin/ls", "../ls", ""] {
            assert_eq!(
                validate(&call(program, &[])),
                Err(ShellRefusal::ProgramNotAbsolute),
                "{program:?}"
            );
        }
        // The program is kept exactly as given: no lookup, no canonicalising.
        let exec = validate(&call(&abs("/opt/tool/../bin/ls"), &[])).unwrap();
        assert_eq!(exec.program, PathBuf::from(abs("/opt/tool/../bin/ls")));
    }

    #[test]
    fn relative_cwd_is_refused() {
        for cwd in ["tmp", "./tmp", "../tmp", ""] {
            assert_eq!(
                validate(&with(call(&abs("/bin/ls"), &[]), "cwd", json!(cwd))),
                Err(ShellRefusal::CwdNotAbsolute),
                "{cwd:?}"
            );
        }
    }

    #[test]
    fn oversized_argv_or_env_is_refused_against_the_supervisor_budget() {
        let program = abs("/bin/echo");

        // Per-argument limit: exactly at the limit is accepted.
        let at = "a".repeat(MAX_PROCESS_ARG_BYTES);
        assert!(validate(&call(&program, &[&at])).is_ok());
        let over = "a".repeat(MAX_PROCESS_ARG_BYTES + 1);
        assert_eq!(
            validate(&call(&program, &[&over])),
            Err(ShellRefusal::ArgTooLarge)
        );

        // Total limit over program + args: exactly at the limit is accepted.
        let rest = MAX_PROCESS_TOTAL_ARG_BYTES - program.len() - MAX_PROCESS_ARG_BYTES;
        let second = "b".repeat(rest);
        assert!(validate(&call(&program, &[&at, &second])).is_ok());
        let second = "b".repeat(rest + 1);
        assert_eq!(
            validate(&call(&program, &[&at, &second])),
            Err(ShellRefusal::ArgvBudgetExceeded)
        );

        // The environment counts as `key=value` toward the same total.
        let value_len = MAX_PROCESS_TOTAL_ARG_BYTES - program.len() - "K=".len();
        let env_at = with(
            call(&program, &[]),
            "env",
            json!({"K": "v".repeat(value_len)}),
        );
        assert!(validate(&env_at).is_ok());
        let env_over = with(
            call(&program, &[]),
            "env",
            json!({"K": "v".repeat(value_len + 1)}),
        );
        assert_eq!(validate(&env_over), Err(ShellRefusal::ArgvBudgetExceeded));
    }

    #[test]
    fn timeout_and_capture_bounds_are_exact() {
        assert_eq!(MAX_SHELL_TIMEOUT, Duration::from_secs(600));
        assert_eq!(MAX_SHELL_CAPTURE_BYTES, 1_048_576);
        let base = call(&abs("/bin/ls"), &[]);
        let max_ms = MAX_SHELL_TIMEOUT.as_millis() as u64;

        for (ms, ok) in [(0, false), (1, true), (max_ms, true), (max_ms + 1, false)] {
            let got = validate(&with(base.clone(), "timeout_ms", json!(ms)));
            if ok {
                assert_eq!(got.unwrap().timeout, Duration::from_millis(ms));
            } else {
                assert_eq!(got, Err(ShellRefusal::TimeoutOutOfRange), "{ms}");
            }
        }

        let max = MAX_SHELL_CAPTURE_BYTES as u64;
        for (bytes, ok) in [(0, false), (1, true), (max, true), (max + 1, false)] {
            let got = validate(&with(base.clone(), "capture_bytes", json!(bytes)));
            if ok {
                assert_eq!(got.unwrap().capture_bytes as u64, bytes);
            } else {
                assert_eq!(got, Err(ShellRefusal::CaptureOutOfRange), "{bytes}");
            }
        }
        assert_eq!(
            validate(&with(base, "timeout_ms", json!(-1))),
            Err(ShellRefusal::Malformed)
        );
    }

    #[test]
    fn nul_bytes_and_bad_env_keys_are_refused() {
        let base = call(&abs("/bin/ls"), &[]);
        let nul_cases = [
            call(&abs("/bin/l\0s"), &[]),
            call(&abs("/bin/ls"), &["a\0b"]),
            with(base.clone(), "cwd", json!(abs("/tm\0p"))),
            with(base.clone(), "env", json!({"K\0": "v"})),
            with(base.clone(), "env", json!({"K": "v\0"})),
        ];
        for case in nul_cases {
            assert_eq!(validate(&case), Err(ShellRefusal::NulByte), "{case}");
        }
        for env in [json!({"": "v"}), json!({"A=B": "v"}), json!({"=A": "v"})] {
            assert_eq!(
                validate(&with(base.clone(), "env", env.clone())),
                Err(ShellRefusal::InvalidEnvKey),
                "{env}"
            );
        }
        // `=` in a value is fine.
        assert!(validate(&with(base, "env", json!({"A": "b=c"}))).is_ok());
    }

    #[test]
    fn unknown_fields_are_refused() {
        let base = call(&abs("/bin/ls"), &[]);
        assert_eq!(
            validate(&with(base.clone(), "stdin", json!("x"))),
            Err(ShellRefusal::Malformed)
        );
        assert_eq!(
            validate(&with(base.clone(), "args", json!("-la"))),
            Err(ShellRefusal::Malformed)
        );
        for required in ["program", "args", "cwd", "timeout_ms", "capture_bytes"] {
            let mut missing = base.clone();
            missing.as_object_mut().unwrap().remove(required);
            assert_eq!(
                validate(&missing),
                Err(ShellRefusal::Malformed),
                "{required}"
            );
        }
    }
}
