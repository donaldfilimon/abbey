//! Honesty surfaces for approved-but-unavailable and out-of-scope capabilities.
//!
//! Four product expansions are Proposed; shipped-edition allowlist bypass is OOS.

use crate::claims;
use crate::os_control;
use crate::platform;
use anyhow::{Result, bail};
use std::fmt::Write as _;

#[derive(Debug, Clone, Copy)]
struct Topic {
    key: &'static str,
    title: &'static str,
    current: &'static [&'static str],
    status: claims::Status,
    unavailable: &'static str,
    instead: &'static str,
    refuse: &'static str,
}

const TOPICS: &[Topic] = &[
    Topic {
        key: "lora",
        title: "LoRA / fine-tuning",
        current: &[
            "abbey learn review|stats|export  — train_candidate curation only",
            "activity + corrections stay provenance substrate — not adapters",
        ],
        status: claims::Status::Proposed,
        unavailable: "fine-tuning / LoRA runners or any weight update inside Abbey",
        instead: "abbey learn review · abbey learn-stats",
        refuse: "lora",
    },
    Topic {
        key: "weights",
        title: "local model weights",
        current: &[
            "Max/Gemma are cursor-agent (or fm) model-alias bindings",
            "ABBEY_BACKEND=fm → macOS 26+ Foundation Models (on-device, not bundled Qwen)",
            "vision/media: path attach + agent/MCP gen (`abbey vision`)",
        ],
        status: claims::Status::Proposed,
        unavailable: "local Qwen/Gemma weights, Abbey-own trained weights, or in-process VLM",
        instead: "abbey role · ABBEY_BACKEND=fm · abbey vision",
        refuse: "weights",
    },
    Topic {
        key: "accel",
        title: "NPU / TPU / GPU runtime",
        current: &[
            "abbey platform compute — host presence inventory (report-only)",
            "abbey accel verify — numerical kernels on Metal, each checked against the CPU oracle (behind `--features accel`; no placement or speedup claim)",
            "multi-thread subagents (--jobs) — CPU process/thread fan-out",
        ],
        status: claims::Status::Proposed,
        unavailable: "GPU/NPU/TPU compilation, training, or inference scheduled by Abbey",
        instead: "abbey accel verify · abbey platform compute · abbey compute",
        refuse: "npu",
    },
    Topic {
        key: "shell",
        title: "unrestricted OS / shell",
        current: &[
            "abbey os allowlist|policy — named allowlist only",
            "abbey os execute <cmd> --confirm — never without --confirm",
        ],
        status: claims::Status::OutOfScope,
        unavailable: "autonomous OS / unrestricted shell / allowlist bypass",
        instead: "abbey allowlist · abbey os execute --confirm …",
        refuse: "shell",
    },
    Topic {
        key: "host",
        title: "MCP / ACP host / tool runtime",
        current: &[
            "abbey mcp serve — Abbey's own read-only MCP server over stdio and unauthenticated loopback-only HTTP",
            "abbey mcp|acp — inventory + peer launch (stdio for real hosts)",
            "abbey runtime — who executes what during a turn",
            "--approve-mcps — cursor-agent auto-approves MCP tools (not Abbey)",
        ],
        status: claims::Status::Proposed,
        unavailable: "Abbey as an MCP/ACP *client-host* consuming external servers, or an in-process tool runtime",
        instead: "abbey mcp serve · abbey runtime · abbey mcp · abbey acp · --approve-mcps",
        refuse: "mcp-host",
    },
];

fn topic(key: &str) -> Option<&'static Topic> {
    let k = key.to_ascii_lowercase();
    TOPICS.iter().find(|t| {
        t.key == k
            || matches!(
                (t.key, k.as_str()),
                ("lora", "finetune" | "fine-tune" | "fine_tune" | "train")
                    | ("weights", "qwen" | "gemma" | "local-weights" | "own-model")
                    | (
                        "accel",
                        "npu" | "tpu" | "gpu" | "cuda" | "metal" | "ane" | "accelerator"
                    )
                    | (
                        "shell",
                        "os" | "unrestricted" | "allowlist-bypass" | "yolo-shell"
                    )
                    | (
                        "host",
                        "mcp" | "acp" | "mcp-host" | "acp-host" | "runtime" | "tools"
                    )
            )
    })
}

fn print_topic(t: &Topic) -> Result<i32> {
    crate::output::print(render_topic(t))?;
    Ok(0)
}

fn render_topic(t: &Topic) -> String {
    let mut text = format!("abbey {} — {} (honest)\n\nCurrent:\n", t.key, t.title);
    for line in t.current {
        let _ = writeln!(text, "  · {line}");
    }
    let mark = if t.status == claims::Status::Proposed {
        "·"
    } else {
        "✗"
    };
    let _ = write!(
        text,
        "\n{}:\n  {mark} {}\n\ninstead: {}\nrefuse:  abbey {} refuse · abbey claims refuse {}\n",
        t.status.label(),
        t.unavailable,
        t.instead,
        t.key,
        t.refuse,
    );
    text
}

pub fn print_index() -> Result<i32> {
    println!("abbey oos — deferred-capability honesty index (not implementation)\n");
    println!(
        "{:<10} {:<28} {:<14} Current substitute",
        "cmd", "topic", "status"
    );
    for t in TOPICS {
        let sub = t.instead.split('·').next().unwrap_or(t.instead).trim();
        println!(
            "{:<10} {:<28} {:<14} {sub}",
            t.key,
            t.title,
            t.status.label()
        );
    }
    println!(
        "\nusage:  abbey oos <lora|weights|accel|shell|host>\n\
         \x20       abbey lora|weights|accel|shell|host [status|refuse]\n\
         refuse: abbey claims refuse lora|weights|npu|shell|mcp-host  (exit 2)\n\
         rule:   these verbs must fail honestly — never silent success"
    );
    Ok(0)
}

pub fn dispatch_topic(key: &str, args: &[String]) -> Result<i32> {
    let Some(t) = topic(key) else {
        bail!("unknown oos topic `{key}` — try: lora|weights|accel|shell|host");
    };
    match args.first().map(String::as_str) {
        None | Some("status") | Some("show") | Some("info") => print_topic(t),
        Some("refuse") | Some("no") | Some("deny") => claims::refuse(t.refuse),
        Some("detect") | Some("compute") if t.key == "accel" => platform::print_compute(),
        // The one accelerator verb that is not deferred: real kernel execution
        // with CPU-oracle parity, behind `--features accel`. Without the
        // feature it refuses with exit 2 rather than silently no-opping.
        Some("verify") | Some("kernel") | Some("kernels") if t.key == "accel" => {
            crate::accel::verify_command(&args[1..])
        }
        Some("allowlist") | Some("policy") if t.key == "shell" => {
            os_control::run_os(&["allowlist".into()], false)
        }
        Some("matrix") if t.key == "host" => crate::surfaces::print_runtime_matrix(),
        Some("-h") | Some("--help") => {
            println!(
                "abbey {k} — {title}\n\
                 usage: abbey {k} [status|refuse{extra}]\n\
                 {status}: {unavailable}\n\
                 instead: {instead}",
                k = t.key,
                title = t.title,
                extra = match t.key {
                    "accel" => "|detect|verify",
                    "shell" => "|allowlist",
                    "host" => "|matrix",
                    _ => "",
                },
                status = t.status.label(),
                unavailable = t.unavailable,
                instead = t.instead,
            );
            Ok(0)
        }
        Some(other) => bail!(
            "unknown {k} subcommand `{other}` — try: status|refuse",
            k = t.key
        ),
    }
}

pub fn dispatch_oos(args: &[String]) -> Result<i32> {
    match args.first().map(String::as_str) {
        None | Some("list") | Some("index") | Some("status") => print_index(),
        Some("-h") | Some("--help") => {
            println!(
                "abbey oos — deferred-capability honesty surfaces\n\
                 \n\
                 usage:\n\
                   abbey oos                         # index\n\
                   abbey oos <lora|weights|accel|shell|host>\n\
                   abbey oos <topic> refuse\n\
                 \n\
                 aliases: abbey lora · weights · accel · shell · host"
            );
            Ok(0)
        }
        Some(key) => {
            let rest: Vec<String> = args.iter().skip(1).cloned().collect();
            dispatch_topic(key, &rest)
        }
    }
}

pub fn status_lines() -> Vec<String> {
    vec![
        "lora:       learn curation only — LoRA pipeline Proposed (`abbey lora`)".into(),
        "weights:    Max/Gemma bindings · fm — local weights Proposed (`abbey weights`)".into(),
        "accel:      host detect + verified Metal kernels (--features accel) — compile/train/infer Proposed (`abbey accel`)".into(),
        "shell:      allowlist + --confirm — unrestricted OS OOS (`abbey shell`)".into(),
        "host:       read-only stdio + loopback HTTP — external-server client-host Proposed (`abbey host`)".into(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_five_topics_resolve() {
        for key in ["lora", "weights", "accel", "shell", "host"] {
            assert!(topic(key).is_some(), "missing topic {key}");
        }
        assert_eq!(topic("npu").map(|t| t.key), Some("accel"));
        assert_eq!(topic("unrestricted").map(|t| t.key), Some("shell"));
        assert_eq!(topic("mcp-host").map(|t| t.key), Some("host"));
        assert_eq!(topic("finetune").map(|t| t.key), Some("lora"));
    }

    #[test]
    fn refuse_paths_exit_two() {
        assert_eq!(dispatch_topic("lora", &["refuse".into()]).unwrap(), 2);
        assert_eq!(dispatch_topic("weights", &["refuse".into()]).unwrap(), 2);
        assert_eq!(dispatch_topic("accel", &["refuse".into()]).unwrap(), 2);
        assert_eq!(dispatch_topic("shell", &["refuse".into()]).unwrap(), 2);
        assert_eq!(dispatch_topic("host", &["refuse".into()]).unwrap(), 2);
    }

    #[test]
    fn topic_statuses_match_the_canonical_claim_migration() {
        for key in ["lora", "weights", "accel", "host"] {
            assert_eq!(topic(key).unwrap().status, claims::Status::Proposed);
        }
        assert_eq!(topic("shell").unwrap().status, claims::Status::OutOfScope);
    }

    #[test]
    fn rendered_topic_statuses_match_the_canonical_claim_migration() {
        for key in ["lora", "weights", "accel", "host"] {
            let rendered = render_topic(topic(key).unwrap());
            assert!(rendered.contains("\nProposed:\n"), "{key}: {rendered}");
            assert!(!rendered.contains("\nOut of scope:\n"));
        }
        let shell = render_topic(topic("shell").unwrap());
        assert!(shell.contains("\nOut of scope:\n"));
    }
}
