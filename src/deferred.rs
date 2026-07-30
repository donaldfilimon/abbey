//! Honesty surfaces for explicitly Out-of-scope capabilities.
//!
//! | Ask | Current substitute | Out of scope |
//! |-----|--------------------|--------------|
//! | LoRA / fine-tune | `learn review|stats|export` | weight updates in Abbey |
//! | Local weights | Max/Gemma bindings · `ABBEY_BACKEND=fm` | bundled Qwen/Gemma / own weights |
//! | NPU/TPU | `platform compute` detect | compile / train / infer on accelerators |
//! | Unrestricted OS | allowlist + `--confirm` | autonomous unrestricted shell |
//! | MCP/ACP host | inventory + `--approve-mcps` | Abbey as tool/MCP/ACP runtime |

use crate::claims;
use crate::os_control;
use crate::platform;
use anyhow::{Result, bail};

#[derive(Debug, Clone, Copy)]
struct Topic {
    key: &'static str,
    title: &'static str,
    current: &'static [&'static str],
    oos: &'static str,
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
        oos: "fine-tuning / LoRA runners or any weight update inside Abbey",
        instead: "abbey learn review · abbey learn stats",
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
        oos: "local Qwen/Gemma weights, Abbey-own trained weights, or in-process VLM",
        instead: "abbey role · ABBEY_BACKEND=fm · abbey vision",
        refuse: "weights",
    },
    Topic {
        key: "accel",
        title: "NPU / TPU / GPU runtime",
        current: &[
            "abbey platform compute — host presence inventory (report-only)",
            "multi-thread subagents (--jobs) — CPU process/thread fan-out",
        ],
        oos: "GPU/NPU/TPU compilation, training, or inference scheduled by Abbey",
        instead: "abbey platform compute · abbey compute",
        refuse: "npu",
    },
    Topic {
        key: "shell",
        title: "unrestricted OS / shell",
        current: &[
            "abbey os allowlist|policy — named allowlist only",
            "abbey os execute <cmd> --confirm — never without --confirm",
        ],
        oos: "autonomous OS / unrestricted shell / allowlist bypass",
        instead: "abbey os <allowlisted> --confirm",
        refuse: "shell",
    },
    Topic {
        key: "host",
        title: "MCP / ACP host / tool runtime",
        current: &[
            "abbey mcp|acp — inventory + peer launch (stdio for real hosts)",
            "abbey runtime — who executes what during a turn",
            "--approve-mcps — cursor-agent auto-approves MCP tools (not Abbey)",
        ],
        oos: "Abbey as MCP host, ACP host, or in-process tool runtime",
        instead: "abbey runtime · abbey mcp · abbey acp · --approve-mcps",
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
    println!("abbey {} — {} (honest)\n", t.key, t.title);
    println!("Current:");
    for line in t.current {
        println!("  · {line}");
    }
    println!();
    println!("Out of scope:");
    println!("  ✗ {}", t.oos);
    println!();
    println!("instead: {}", t.instead);
    println!(
        "refuse:  abbey {} refuse · abbey claims refuse {}",
        t.key, t.refuse
    );
    Ok(0)
}

pub fn print_index() -> Result<i32> {
    println!("abbey oos — Out-of-scope honesty index (not an implementation plan)\n");
    println!("{:<10} {:<28} Current substitute", "cmd", "topic");
    for t in TOPICS {
        let sub = t.instead.split('·').next().unwrap_or(t.instead).trim();
        println!("{:<10} {:<28} {sub}", t.key, t.title);
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
        Some("allowlist") | Some("policy") if t.key == "shell" => {
            os_control::run_os(&["allowlist".into()], false)
        }
        Some("matrix") if t.key == "host" => crate::surfaces::print_runtime_matrix(),
        Some("-h") | Some("--help") => {
            println!(
                "abbey {k} — {title}\n\
                 usage: abbey {k} [status|refuse{extra}]\n\
                 OOS: {oos}\n\
                 instead: {instead}",
                k = t.key,
                title = t.title,
                extra = match t.key {
                    "accel" => "|detect",
                    "shell" => "|allowlist",
                    "host" => "|matrix",
                    _ => "",
                },
                oos = t.oos,
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
                "abbey oos — Out-of-scope honesty surfaces\n\
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
        "lora:       learn curation only — LoRA runners OOS (`abbey lora`)".into(),
        "weights:    Max/Gemma bindings · fm — local weights OOS (`abbey weights`)".into(),
        "accel:      host detect only — NPU/TPU runtime OOS (`abbey accel`)".into(),
        "shell:      allowlist + --confirm — unrestricted OS OOS (`abbey shell`)".into(),
        "host:       inventory + approve-mcps — MCP/ACP host OOS (`abbey host`)".into(),
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
}
