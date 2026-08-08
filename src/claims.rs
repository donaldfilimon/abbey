//! Claims gate — Current / Proposed / Out of scope.
//!
//! Single machine-readable source for honesty. Docs and `AGENTS.md` stay the
//! human table; this module powers `abbey claims` and refuse paths so Proposed
//! items (production multi-host) and OOS items (LoRA, local weights) are never
//! silently implied by a missing error.

use crate::output;
use anyhow::{Result, bail};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Current,
    Proposed,
    OutOfScope,
}

impl Status {
    pub fn label(self) -> &'static str {
        match self {
            Self::Current => "Current",
            Self::Proposed => "Proposed",
            Self::OutOfScope => "Out of scope",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Claim {
    pub name: &'static str,
    pub status: Status,
    /// Short note — for Proposed/OOS: why deferred; for Current: caveat if any.
    pub note: &'static str,
    /// What to use instead (Current substitute), if any.
    pub instead: Option<&'static str>,
}

/// Canonical gate. Keep in sync with `AGENTS.md` claims table.
pub const CLAIMS: &[Claim] = &[
    // —— Current ——
    Claim {
        name: "cursor-agent backend (CLI/TUI)",
        status: Status::Current,
        note: "default executor; alternatives: ABBEY_BACKEND=fm|grok|abi",
        instead: None,
    },
    Claim {
        name: "Grok/Codex/Claude surface parity",
        status: Status::Current,
        note: "partial — polish continues; not a reimplementation of those runtimes",
        instead: None,
    },
    Claim {
        name: "Persona Abbey/Aviva/Abi",
        status: Status::Current,
        note: "via abi-ai contracts",
        instead: None,
    },
    Claim {
        name: "Max/Gemma role bindings",
        status: Status::Current,
        note: "model-alias bindings — not local Qwen/Gemma weights",
        instead: None,
    },
    Claim {
        name: "SQLite memory + self-learn",
        status: Status::Current,
        note: "STM/LTM/activity/train_candidate + learn pipeline",
        instead: None,
    },
    Claim {
        name: "3-D memory map (topic × recency × consolidation)",
        status: Status::Current,
        note: "deterministic axes — not a learned embedding space",
        instead: None,
    },
    Claim {
        name: "WDBX in-process memory",
        status: Status::Current,
        note: "behind --features wdbx (off by default); flock-guarded",
        instead: None,
    },
    Claim {
        name: "Hybrid loop / route audit / learn review",
        status: Status::Current,
        note: "Gemma→Max correlated; confidence/alt/fb audit-only; train curation",
        instead: None,
    },
    Claim {
        name: "Multi-subagent + local PATH peers",
        status: Status::Current,
        note: "abbey subagents — same-host peers, not multi-node mesh",
        instead: None,
    },
    Claim {
        name: "Goal-driven improve loop (local subagents + check.sh bar)",
        status: Status::Current,
        note: "abbey improve — ledger pick + bounded --confirm apply; not multi-node / not unrestricted OS",
        instead: Some("abbey improve status|plan|run --confirm"),
    },
    Claim {
        name: "MCP/ACP inventory + voice + highlight + media/imagine",
        status: Status::Current,
        note: "not an MCP/ACP host; no local vision/gen/voice weights",
        instead: None,
    },
    Claim {
        name: "On-device backend (ABBEY_BACKEND=fm)",
        status: Status::Current,
        note: "macOS 26+ Foundation Models CLI; account/gen refuse; local MCP inventory remains available",
        instead: None,
    },
    Claim {
        name: "abi backend (ABBEY_BACKEND=abi) — no cursor-agent required",
        status: Status::Current,
        note: "one-shot `abi complete`: deterministic persona-template locally by default; \
               bare claude-* / live|anthropic opt into abi's Anthropic transport (abi credentials); \
               cursor role/thinking bindings stay local (no silent --live); needs a real `abi` binary; \
               account/gen refuse; local MCP inventory remains available; Abbey-side transcript continuity (bounded context prefix — \
               abi itself stays stateless); default backend selectable via config `backend` key",
        instead: None,
    },
    Claim {
        name: "linux/macos/windows/unix primary host targets (portable surfaces)",
        status: Status::Current,
        note: "CLI/TUI/sqlite/WDBX-lock/subagents/os-allowlist; PATHEXT which_bin; voice+fm macOS-only",
        instead: Some("abbey platform · abbey platform paths"),
    },
    Claim {
        name: "multi-threaded subagent fan-out (--jobs)",
        status: Status::Current,
        note: "std::thread parallelism sized from available_parallelism — not GPU kernels",
        instead: Some("abbey platform threads · ABBEY_SUBAGENT_JOBS"),
    },
    Claim {
        name: "WDBX cross-process lock (Unix + Windows)",
        status: Status::Current,
        note: "fs4 advisory lock (flock / LockFileEx); behind --features wdbx",
        instead: None,
    },
    Claim {
        name: "GPU/NPU/TPU presence detection (report-only)",
        status: Status::Current,
        note: "abbey platform compute — inventory only, not Abbey accelerators",
        instead: Some("abbey platform · abbey compute"),
    },
    Claim {
        name: "CoT transcript viewer (`abbey cot`)",
        status: Status::Current,
        note: "saves/displays structured reason output — not an Abbey CoT engine",
        instead: Some("abbey cot show|run · abbey reason"),
    },
    Claim {
        name: "tool responsibility matrix (`abbey runtime`)",
        status: Status::Current,
        note: "documents who executes what; Abbey is not the tool host",
        instead: Some("abbey runtime · --approve-mcps"),
    },
    Claim {
        name: "OOS honesty surfaces (`abbey oos` / lora|weights|accel|shell|host)",
        status: Status::Current,
        note: "status + refuse for deferred capabilities — does not implement them",
        instead: Some("abbey oos · abbey claims refuse …"),
    },
    Claim {
        name: "lexical similarity search over memory (feature-hash cosine)",
        status: Status::Current,
        note: "abi-ai n-gram hash + cosine at query time — independent of learned providers",
        instead: Some("abbey memory similar <query> | --id <id>"),
    },
    Claim {
        name: "semantic / learned memory embedding space",
        status: Status::Current,
        note: "opt-in apple|openai provider; space-isolated SQLite/WDBX persistence; \
               Apple paraphrase ranking locally verified; remote live call unverified",
        instead: Some("abbey memory semantic · memory embed status|--all"),
    },
    Claim {
        name: "memory filter by source / timestamp / project",
        status: Status::Current,
        note: "shared exact metadata plus inclusive RFC 3339 bounds on SQLite and WDBX",
        instead: Some("memory search|similar|semantic|map|near|export filter flags"),
    },
    Claim {
        name: "ABI WDBX authenticated local multi-process proof",
        status: Status::Current,
        note: "abbey mesh local-demo on Unix; 3..=9 loopback ABI processes; not production multi-host",
        instead: Some("ABBEY_ABI_BIN=<real binary> abbey mesh local-demo --nodes 3"),
    },
    // —— Proposed ——
    Claim {
        name: "production multi-host · multi-GPU · shared compute mesh",
        status: Status::Proposed,
        note: "the authenticated local multi-process proof does not establish separate-host deployment",
        instead: Some("abbey mesh local-demo · abbey subagents --peers (same host)"),
    },
    // —— Out of scope ——
    Claim {
        name: "fine-tuning / LoRA runners",
        status: Status::OutOfScope,
        note: "train_candidate is curation substrate only — no weight updates in Abbey",
        instead: Some("abbey lora · abbey learn-review · abbey learn-stats"),
    },
    Claim {
        name: "local Qwen / Gemma weights",
        status: Status::OutOfScope,
        note: "Max/Gemma are cursor-agent (or fm) bindings, not bundled weights",
        instead: Some("abbey weights · ABBEY_BACKEND=fm"),
    },
    Claim {
        name: "Abbey as her own trained model (own weights)",
        status: Status::OutOfScope,
        note: "product identity ≠ local foundation-model training loop",
        instead: Some("abbey weights"),
    },
    Claim {
        name: "GPU/NPU/TPU compilation, training, or inference in Abbey",
        status: Status::OutOfScope,
        note: "presence inventory only — Abbey does not schedule accelerator kernels",
        instead: Some("abbey accel · abbey platform compute"),
    },
    Claim {
        name: "autonomous OS / unrestricted shell",
        status: Status::OutOfScope,
        note: "os_control allowlist + --confirm is a safety invariant",
        instead: Some("abbey allowlist · abbey os execute --confirm · abbey shell"),
    },
    Claim {
        name: "Abbey as MCP host / ACP host / tool runtime",
        status: Status::OutOfScope,
        note: "inventory + launch only; tools run inside cursor-agent during a turn",
        instead: Some("abbey host · abbey runtime · abbey mcp|acp · --approve-mcps"),
    },
    Claim {
        name: "Abbey-owned chain-of-thought engine / interactive CoT UI",
        status: Status::OutOfScope,
        note: "reason uses Cursor thinking models; cot is a transcript viewer only",
        instead: Some("abbey cot show · abbey reason"),
    },
    Claim {
        name: "fake cost / token accounting",
        status: Status::OutOfScope,
        note: "/cost stays N/A for cursor-agent",
        instead: Some("Cursor account dashboard"),
    },
    Claim {
        name: "cloud TTS/STT SaaS · local neural voice weights",
        status: Status::OutOfScope,
        note: "macOS say Premium/Enhanced + on-device Speech only",
        instead: Some("abbey voice · System Settings → Spoken Content"),
    },
    Claim {
        name: "local vision / generation weights",
        status: Status::OutOfScope,
        note: "path attach + agent/MCP tools write files — Abbey does not embed pixels",
        instead: Some("abbey --image · abbey imagine"),
    },
    Claim {
        name: "reimplement Grok/Codex/Claude runtimes",
        status: Status::OutOfScope,
        note: "surface parity only",
        instead: None,
    },
];

pub fn by_status(status: Status) -> impl Iterator<Item = &'static Claim> {
    CLAIMS.iter().filter(move |c| c.status == status)
}

pub fn lookup(keyword: &str) -> Vec<&'static Claim> {
    let key = keyword.to_ascii_lowercase();
    CLAIMS
        .iter()
        .filter(|c| {
            c.name.to_ascii_lowercase().contains(&key) || c.note.to_ascii_lowercase().contains(&key)
        })
        .collect()
}

/// Print the gate. `filter`: None = all sections; Some("proposed"|"oos"|"current"|keyword).
pub fn print_claims(filter: Option<&str>) -> Result<i32> {
    let filter = filter.map(str::trim).filter(|s| !s.is_empty());
    match filter.map(str::to_ascii_lowercase).as_deref() {
        None | Some("all") => {
            print_section(Status::Current);
            println!();
            print_section(Status::Proposed);
            println!();
            print_section(Status::OutOfScope);
            print_footer();
        }
        Some("current" | "shipped") => print_section(Status::Current),
        Some("proposed" | "prop" | "roadmap") => {
            print_section(Status::Proposed);
            print_footer();
        }
        Some("oos" | "out" | "out-of-scope" | "deferred") => {
            print_section(Status::OutOfScope);
            print_footer();
        }
        Some(key) => {
            let hits = lookup(key);
            if hits.is_empty() {
                bail!("no claims matching `{key}` — try: abbey claims proposed|oos|current");
            }
            println!("abbey claims — matches for `{key}`\n");
            for c in hits {
                print_claim(c);
            }
            print_footer();
        }
    }
    Ok(0)
}

fn print_section(status: Status) {
    let title = match status {
        Status::Current => "Current (shipped)",
        Status::Proposed => "Proposed (designed — not claimed live)",
        Status::OutOfScope => "Out of scope (explicitly deferred)",
    };
    println!("abbey claims — {title}");
    for c in by_status(status) {
        print_claim(c);
    }
}

fn print_claim(c: &Claim) {
    let mark = match c.status {
        Status::Current => "✓",
        Status::Proposed => "·",
        Status::OutOfScope => "✗",
    };
    let _ = output::println(format!("  {mark} {}", c.name));
    let _ = output::println(format!("      {}", c.note));
    if let Some(alt) = c.instead {
        let _ = output::println(format!("      instead: {alt}"));
    }
}

fn print_footer() {
    println!(
        "\nrefuse:  abbey claims refuse <lora|multinode|npu|…>  (exit 2)\n\
         docs:    AGENTS.md claims gate · docs/architecture.md · docs/identity.md\n\
         rule:    Proposed/OOS verbs must fail honestly — never silent success"
    );
}

/// Map a user verb to a Proposed/OOS claim and refuse with exit 2.
pub fn refuse(verb: &str) -> Result<i32> {
    let key = verb.trim().to_ascii_lowercase();
    let (claim_key, detail) = match key.as_str() {
        "embed" | "embedding" | "embeddings" | "semantic" | "vector" | "vectors" => {
            eprintln!(
                "abbey: semantic embeddings are Current when an explicit apple|openai provider \
                 is configured; use `abbey memory embed status`"
            );
            return Ok(0);
        }
        "lora" | "finetune" | "fine-tune" | "fine_tune" | "train-weights" => (
            "lora",
            "Fine-tuning / LoRA is Out of scope. train_candidate is curation only.",
        ),
        "multinode" | "multi-node" | "cluster" | "mesh" | "multi-gpu" | "distributed-mesh" => (
            "multi-host",
            "Production multi-host / multi-GPU shared compute is Proposed. The authenticated local multi-process proof is Current on Unix hosts.",
        ),
        "npu" | "tpu" | "gpu" | "cuda" | "metal" | "ane" => (
            "GPU/NPU/TPU",
            "GPU/NPU/TPU compilation, training, and inference in Abbey are Out of scope. Host detect is Current.",
        ),
        "vision" | "vlm" | "video-weights" | "local-vision" => (
            "vision",
            "Local vision/video weights are Out of scope. Path attach + agent/MCP gen are Current.",
        ),
        "cot" | "chain-of-thought" | "cot-ui" | "cot-engine" => (
            "CoT",
            "Abbey-owned CoT engine/UI is Out of scope. Transcript viewer + Cursor thinking wrap are Current.",
        ),
        "weights" | "qwen" | "local-gemma" | "own-model" => (
            "weights",
            "Local Qwen/Gemma weights and Abbey-own-weights training are Out of scope.",
        ),
        "cost" | "tokens" | "billing" => (
            "cost",
            "Fake cost/token accounting is Out of scope. /cost is N/A.",
        ),
        "mcp-host" | "acp-host" | "host" | "tool-runtime" | "tool-host" => (
            "MCP host",
            "Abbey is not an MCP or ACP host — inventory/launch only.",
        ),
        "shell" | "unrestricted" | "os-unrestricted" | "allowlist-bypass" | "yolo-shell" => (
            "unrestricted",
            "Unrestricted OS / autonomous shell is Out of scope. Allowlist + --confirm is Current.",
        ),
        "accel" | "accelerator" | "accelerators" => (
            "GPU/NPU/TPU",
            "GPU/NPU/TPU compilation, training, and inference in Abbey are Out of scope. Host detect is Current.",
        ),
        other => {
            eprintln!(
                "abbey: unknown refuse topic `{other}`\n\
                 try: lora · multinode · npu · weights · shell · cost · mcp-host"
            );
            return Ok(2);
        }
    };

    let hits: Vec<_> = lookup(claim_key)
        .into_iter()
        .filter(|c| c.status != Status::Current)
        .collect();
    eprintln!("abbey: refused — {detail}");
    for c in &hits {
        eprintln!("  [{}] {}", c.status.label(), c.name);
        if let Some(alt) = c.instead {
            eprintln!("  instead: {alt}");
        }
    }
    Ok(2)
}

pub fn dispatch(args: &[String]) -> Result<i32> {
    if args.is_empty()
        || matches!(
            args.first().map(String::as_str),
            Some("list" | "show" | "all" | "-h" | "--help")
        ) && args.len() <= 1
    {
        if matches!(args.first().map(String::as_str), Some("-h" | "--help")) {
            println!(
                "abbey claims — Current / Proposed / Out of scope gate\n\
                 \n\
                 usage:\n\
                   abbey claims              # full gate\n\
                   abbey claims proposed     # Proposed only\n\
                   abbey claims oos          # Out of scope only\n\
                   abbey claims current      # shipped\n\
                   abbey claims <keyword>    # search\n\
                   abbey claims refuse <topic>\n\
                 \n\
                 topics: lora · multinode · npu · weights · shell · cost · mcp-host\n\
                 note: embeddings are Current; `claims refuse embeddings` reports that status"
            );
            return Ok(0);
        }
        return print_claims(None);
    }

    match args[0].as_str() {
        "refuse" | "no" | "deny" => {
            let topic = args.get(1).map(String::as_str).unwrap_or("");
            if topic.is_empty() {
                bail!("usage: abbey claims refuse <lora|multinode|npu|…>");
            }
            refuse(topic)
        }
        "proposed" | "prop" | "roadmap" => print_claims(Some("proposed")),
        "oos" | "out" | "out-of-scope" | "deferred" => print_claims(Some("oos")),
        "current" | "shipped" => print_claims(Some("current")),
        other => print_claims(Some(other)),
    }
}

pub fn status_line() -> String {
    let cur = by_status(Status::Current).count();
    let prop = by_status(Status::Proposed).count();
    let oos = by_status(Status::OutOfScope).count();
    format!("claims:    {cur} Current · {prop} Proposed · {oos} Out of scope — `abbey claims`")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gate_has_all_three_statuses() {
        assert!(by_status(Status::Current).count() >= 14);
        assert!(by_status(Status::Proposed).next().is_some());
        assert!(by_status(Status::OutOfScope).count() >= 5);
    }

    #[test]
    fn embeddings_are_current_and_explicitly_scoped() {
        let semantic = CLAIMS
            .iter()
            .find(|c| c.name.starts_with("semantic"))
            .expect("semantic embedding claim");
        assert_eq!(semantic.status, Status::Current);
        assert!(semantic.note.contains("opt-in"));
        assert!(semantic.note.contains("remote live call unverified"));
    }

    #[test]
    fn abi_backend_is_current() {
        let claim = CLAIMS
            .iter()
            .find(|c| c.name.starts_with("abi backend"))
            .expect("abi backend claim");
        assert_eq!(claim.status, Status::Current);
        assert!(claim.note.contains("abi complete"));
    }

    #[test]
    fn lora_is_out_of_scope() {
        let hits = lookup("lora");
        assert!(hits.iter().any(|c| c.status == Status::OutOfScope));
    }

    #[test]
    fn refuse_only_rejects_proposed_or_out_of_scope_topics() {
        assert_eq!(refuse("embeddings").unwrap(), 0);
        assert_eq!(refuse("lora").unwrap(), 2);
        assert_eq!(refuse("multinode").unwrap(), 2);
        assert_eq!(refuse("shell").unwrap(), 2);
        assert_eq!(refuse("host").unwrap(), 2);
        assert_eq!(refuse("npu").unwrap(), 2);
        assert_eq!(refuse("weights").unwrap(), 2);
    }

    #[test]
    fn lookup_production_multihost() {
        let hits = lookup("multi-host");
        assert!(hits.iter().any(|c| c.status == Status::Proposed));
    }
}
