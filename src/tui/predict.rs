//! Ranked command prediction for the TUI composer.
//!
//! The live predictor is a local ranker over the slash catalog, cross-CLI
//! aliases, intent phrases, and prompt history. An optional Ollama rerank
//! may boost one catalog name; it is fail-closed, time-bounded, and never
//! required for suggestions to appear.

use std::ffi::OsString;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crate::runtime::supervisor::{
    ProcessSpec, SupervisorLimits, SupervisorOutcome, run_with_checkpoint,
};
use crate::slash::{SLASH_CATALOG, SlashCmd};
use crate::slash_alias::{self, SLASH_ALIASES};

/// Small local tag used only for command rerank. The default generation
/// model (`gemma4:26b-mlx`) is too slow for keystroke prediction.
pub const PREDICT_MODEL: &str = "gemma4:12b-mlx";

const MAX_RESULTS: usize = 8;
const LLM_TIMEOUT: Duration = Duration::from_millis(1_500);
const MAX_LLM_OUTPUT_BYTES: usize = 4 * 1024;
static LLM_IN_FLIGHT: AtomicBool = AtomicBool::new(false);

/// Background Ollama rerank result. Stale generations are ignored.
pub(super) struct LlmHint {
    pub generation: u64,
    pub name: Option<&'static str>,
}

struct InFlightGuard;

impl Drop for InFlightGuard {
    fn drop(&mut self) {
        LLM_IN_FLIGHT.store(false, Ordering::Release);
    }
}

/// Start at most one process-wide rerank. Lexical predictions remain available
/// while an older generation finishes or times out.
pub(super) fn spawn_llm_hint(
    ollama: std::path::PathBuf,
    model: &'static str,
    input: String,
    generation: u64,
) -> Option<std::sync::mpsc::Receiver<LlmHint>> {
    LLM_IN_FLIGHT
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .ok()?;
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _guard = InFlightGuard;
        let name = llm_hint(&ollama, model, &input);
        let _ = tx.send(LlmHint { generation, name });
    });
    Some(rx)
}

/// One ranked suggestion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Prediction {
    pub name: &'static str,
    pub help: &'static str,
    pub origin: &'static str,
    pub via: &'static str,
    pub score: u16,
}

/// Intent phrases that are not catalog names. Longer phrases first.
const INTENTS: &[(&str, &str)] = &[
    ("please fix", "please-fix"),
    ("pull request", "pr"),
    ("security review", "security-review"),
    ("new chat", "new"),
    ("what's wrong", "doctor"),
    ("what is wrong", "doctor"),
    ("write a commit", "commit"),
    ("draft a commit", "commit"),
    ("look at the diff", "review"),
    ("review the", "review"),
    ("remember this", "learn"),
    ("search memory", "memory"),
    ("run tests", "please-fix"),
    ("fix the", "please-fix"),
    ("diagnose", "doctor"),
    ("health", "doctor"),
    ("continue", "continue"),
    ("resume", "continue"),
    ("compact", "compact"),
    ("commit", "commit"),
    ("review", "review"),
    ("doctor", "doctor"),
    ("memory", "memory"),
    ("plan", "plan"),
    ("init", "init"),
];

fn catalog(name: &str) -> Option<&'static SlashCmd> {
    SLASH_CATALOG.iter().find(|c| c.name == name)
}

fn push_unique(out: &mut Vec<Prediction>, pred: Prediction) {
    if out.iter().any(|p| p.name == pred.name) {
        return;
    }
    out.push(pred);
}

/// Rank commands for the current composer text.
pub fn rank(input: &str, history: &[String], llm_boost: Option<&str>) -> Vec<Prediction> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    let slashy = trimmed.starts_with('/');
    let body = trimmed.trim_start_matches('/').trim();
    // A slash command that already has arguments is complete: don't keep the
    // suggestion overlay open over the rest of the line.
    if slashy && body.contains(char::is_whitespace) {
        return Vec::new();
    }
    let first = body
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    let lower = body.to_ascii_lowercase();

    if !slashy && first.len() < 2 && !lower.contains(' ') {
        return Vec::new();
    }

    let mut scored: Vec<Prediction> = Vec::new();

    if slashy || !first.is_empty() {
        for cmd in SLASH_CATALOG {
            if first.is_empty() || cmd.name.starts_with(first.as_str()) {
                let score = if cmd.name == first {
                    1_000
                } else if cmd.name.starts_with(&first) {
                    900u16.saturating_sub(cmd.name.len().saturating_sub(first.len()) as u16)
                } else {
                    continue;
                };
                scored.push(Prediction {
                    name: cmd.name,
                    help: cmd.help,
                    origin: slash_alias::origin_for(cmd.name),
                    via: "prefix",
                    score,
                });
            }
        }
        for alias in SLASH_ALIASES {
            if first.is_empty() || alias.alias.starts_with(first.as_str()) {
                let Some(cmd) = catalog(alias.target) else {
                    continue;
                };
                let score = if alias.alias == first.as_str() {
                    950
                } else {
                    850
                };
                push_unique(
                    &mut scored,
                    Prediction {
                        name: cmd.name,
                        help: cmd.help,
                        origin: alias.origin,
                        via: "alias",
                        score,
                    },
                );
            }
        }
    }

    if !slashy {
        for (phrase, target) in INTENTS {
            if lower.contains(phrase) {
                let Some(cmd) = catalog(target) else {
                    continue;
                };
                push_unique(
                    &mut scored,
                    Prediction {
                        name: cmd.name,
                        help: cmd.help,
                        origin: slash_alias::origin_for(cmd.name),
                        via: "intent",
                        score: 700 + (phrase.len() as u16).min(80),
                    },
                );
            }
        }
        if lower.contains(' ') {
            for cmd in SLASH_CATALOG {
                if cmd.help.to_ascii_lowercase().split_whitespace().any(|w| {
                    w.len() >= 4 && lower.split_whitespace().any(|t| t == w.trim_matches(','))
                }) {
                    push_unique(
                        &mut scored,
                        Prediction {
                            name: cmd.name,
                            help: cmd.help,
                            origin: slash_alias::origin_for(cmd.name),
                            via: "help",
                            score: 520,
                        },
                    );
                }
            }
        }
    }

    for prev in history.iter().rev().take(12) {
        let Some((name, _)) = crate::slash::parse_slash(prev) else {
            continue;
        };
        let Some(canon) = slash_alias::resolve_name(name) else {
            continue;
        };
        if lower.is_empty() || canon.contains(&first) || first.contains(canon) {
            let Some(cmd) = catalog(canon) else {
                continue;
            };
            push_unique(
                &mut scored,
                Prediction {
                    name: cmd.name,
                    help: cmd.help,
                    origin: "history",
                    via: "history",
                    score: 560,
                },
            );
        }
    }

    if let Some(boost) = llm_boost.and_then(slash_alias::resolve_name)
        && let Some(cmd) = catalog(boost)
    {
        if let Some(existing) = scored.iter_mut().find(|p| p.name == cmd.name) {
            existing.score = existing.score.saturating_add(200);
            existing.via = "llm";
        } else {
            scored.push(Prediction {
                name: cmd.name,
                help: cmd.help,
                origin: slash_alias::origin_for(cmd.name),
                via: "llm",
                score: 880,
            });
        }
    }

    scored.sort_by(|a, b| b.score.cmp(&a.score).then(a.name.cmp(b.name)));
    scored.dedup_by(|a, b| a.name == b.name);
    scored.truncate(MAX_RESULTS);
    scored
}

/// Turn the current composer text plus a selected prediction into `/cmd …`.
pub fn accept_text(input: &str, name: &str) -> String {
    let t = input.trim_start();
    if t.starts_with('/') {
        let rest = t
            .split_once(char::is_whitespace)
            .map(|(_, r)| r.trim())
            .unwrap_or("");
        if rest.is_empty() {
            format!("/{name} ")
        } else {
            format!("/{name} {rest}")
        }
    } else {
        let mut parts = t.split_whitespace();
        let first = parts
            .next()
            .unwrap_or("")
            .trim_start_matches('/')
            .to_ascii_lowercase();
        let rest: Vec<&str> = parts.collect();
        if first == name || (!first.is_empty() && name.starts_with(&first)) {
            if rest.is_empty() {
                format!("/{name} ")
            } else {
                format!("/{name} {}", rest.join(" "))
            }
        } else {
            format!("/{name} {}", t.trim())
        }
    }
}

fn parse_llm_name(raw: &str) -> Option<&'static str> {
    let line = raw
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with("Thinking"))?;
    let token = line
        .trim_start_matches('`')
        .trim_end_matches('`')
        .trim_start_matches('/')
        .split_whitespace()
        .next()?
        .trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '-' && c != '?');
    slash_alias::resolve_name(token)
}

/// Bounded one-shot Ollama rerank. Fail-closed: timeout, missing binary,
/// unknown name, and non-zero exit all return `None`.
pub fn llm_hint(ollama: &Path, model: &str, input: &str) -> Option<&'static str> {
    if input.trim().len() < 4 {
        return None;
    }
    if !crate::agent::ollama_lists_model(ollama, model) {
        return None;
    }
    let names: String = SLASH_CATALOG
        .iter()
        .map(|c| c.name)
        .collect::<Vec<_>>()
        .join(", ");
    let clipped: String = input.chars().take(240).collect();
    let prompt = format!(
        "Pick exactly one Abbey slash command for this user text.\n\
         Commands: {names}\n\
         Text: {clipped}\n\
         Reply with only the command name, no slash, no punctuation."
    );
    let spec = ProcessSpec::inherited(
        ollama.to_path_buf(),
        vec![
            OsString::from("run"),
            OsString::from("--nowordwrap"),
            OsString::from("--hidethinking"),
            OsString::from(model),
            OsString::from("--"),
            OsString::from(prompt),
        ],
    );
    let limits = SupervisorLimits {
        timeout: LLM_TIMEOUT,
        terminate_grace: Duration::from_millis(100),
        stdout_bytes: MAX_LLM_OUTPUT_BYTES,
        stderr_bytes: 1024,
        poll_interval: Duration::from_millis(10),
    };
    let SupervisorOutcome::Exited { status, stdout, .. } =
        run_with_checkpoint(&spec, &limits, || false).ok()?
    else {
        return None;
    };
    status
        .success()
        .then(|| parse_llm_name(&String::from_utf8_lossy(&stdout)))
        .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slash_prefix_ranks_help_first() {
        let hits = rank("/he", &[], None);
        assert_eq!(hits[0].name, "help");
        assert_eq!(hits[0].via, "prefix");
    }

    #[test]
    fn natural_language_maps_review_and_fix() {
        let review = rank("review the auth diff", &[], None);
        assert_eq!(review[0].name, "review");
        let fix = rank("please fix the failing tests", &[], None);
        assert_eq!(fix[0].name, "please-fix");
    }

    #[test]
    fn cross_cli_alias_prefix_is_predicted() {
        let exec = rank("/ex", &[], None);
        assert!(
            exec.iter().any(|p| p.name == "ask" && p.via == "alias"),
            "{exec:?}"
        );
        let plugin = rank("plugin", &[], None);
        assert!(plugin.iter().any(|p| p.name == "plugins"));
    }

    #[test]
    fn accept_completes_slash_and_keeps_nl_args() {
        assert_eq!(accept_text("/he", "help"), "/help ");
        assert_eq!(
            accept_text("review the login flow", "review"),
            "/review the login flow"
        );
        assert_eq!(accept_text("review", "review"), "/review ");
    }

    #[test]
    fn llm_parser_accepts_only_catalog_names() {
        assert_eq!(parse_llm_name("/review\n"), Some("review"));
        assert_eq!(parse_llm_name("Thinking...\nplugins"), Some("plugins"));
        assert_eq!(parse_llm_name("not-a-command"), None);
    }

    #[test]
    fn llm_boost_promotes_named_command() {
        let hits = rank("look at this", &[], Some("diff"));
        assert_eq!(hits[0].name, "diff");
        assert_eq!(hits[0].via, "llm");
    }

    #[cfg(unix)]
    #[test]
    fn llm_hint_rejects_output_past_the_capture_bound() {
        use std::os::unix::fs::PermissionsExt as _;

        let path = std::env::temp_dir().join(format!(
            "abbey-predict-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::write(
            &path,
            format!(
                "#!/bin/sh\nif [ \"$1\" = list ]; then printf 'NAME ID SIZE\\n{PREDICT_MODEL} digest 1GB\\n'; exit 0; fi\ni=0\nwhile [ \"$i\" -lt 5000 ]; do printf x; i=$((i + 1)); done\n"
            ),
        )
        .unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();

        assert_eq!(llm_hint(&path, PREDICT_MODEL, "review this change"), None);
        std::fs::remove_file(path).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn llm_hint_tears_down_descendants_that_retain_stdout() {
        use std::os::unix::fs::PermissionsExt as _;

        let path = std::env::temp_dir().join(format!(
            "abbey-predict-descendant-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::write(
            &path,
            format!(
                "#!/bin/sh\nif [ \"$1\" = list ]; then printf 'NAME ID SIZE\\n{PREDICT_MODEL} digest 1GB\\n'; exit 0; fi\nsleep 10 &\nexit 0\n"
            ),
        )
        .unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();

        let started = std::time::Instant::now();
        assert_eq!(llm_hint(&path, PREDICT_MODEL, "review this change"), None);
        assert!(started.elapsed() < Duration::from_secs(2));
        std::fs::remove_file(path).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn llm_hint_does_not_run_a_missing_prediction_model() {
        use std::os::unix::fs::PermissionsExt as _;

        let path = std::env::temp_dir().join(format!(
            "abbey-predict-missing-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        let marker = path.with_extension("ran");
        std::fs::write(
            &path,
            format!(
                "#!/bin/sh\nif [ \"$1\" = list ]; then printf 'NAME ID SIZE\\nother:latest digest 1GB\\n'; exit 0; fi\ntouch '{}'\necho review\n",
                marker.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();

        assert_eq!(llm_hint(&path, PREDICT_MODEL, "review this change"), None);
        assert!(!marker.exists(), "ollama run must not start a pull");
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn history_contributes_recent_slash() {
        let hist = ["/doctor".to_string(), "/commit".to_string()];
        let hits = rank("doc", &hist, None);
        assert!(hits.iter().any(|p| p.name == "doctor"));
    }
}
