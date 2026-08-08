//! Claims gate — Current / Partial / Proposed / Blocked / Out of scope.
//!
//! Single machine-readable source for honesty. Docs and `AGENTS.md` stay the
//! human table; this module powers `abbey claims` and refusal paths so approved
//! roadmap work, externally blocked proof, and deliberately excluded work are
//! never silently implied by a missing error.

use crate::output;
use anyhow::{Result, bail};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Current,
    Partial,
    Proposed,
    Blocked,
    OutOfScope,
}

impl Status {
    pub fn label(self) -> &'static str {
        match self {
            Self::Current => "Current",
            Self::Partial => "Partial",
            Self::Proposed => "Proposed",
            Self::Blocked => "Blocked",
            Self::OutOfScope => "Out of scope",
        }
    }

    fn key(self) -> &'static str {
        match self {
            Self::Current => "current",
            Self::Partial => "partial",
            Self::Proposed => "proposed",
            Self::Blocked => "blocked",
            Self::OutOfScope => "out_of_scope",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Claim {
    pub name: &'static str,
    pub status: Status,
    /// Short note — evidence boundary, blocker, or Current caveat.
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
        status: Status::Partial,
        note: "selected UX aliases and compatible surfaces ship; parity polish continues; not a reimplementation of vendor runtimes",
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
        note: "behind --features wdbx (off by default); fs4 advisory lock (flock / LockFileEx)",
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
        note: "inventory/peer launch and delegated media surfaces; no Abbey-owned tool host or local neural media models yet",
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
        note: "abbey mesh local-demo on Unix; 3..=9 loopback ABI processes; not production multi-VM",
        instead: Some("ABBEY_ABI_BIN=<real binary> abbey mesh local-demo --nodes 3"),
    },
    // —— Proposed (approved roadmap; refusal paths remain fail-closed) ——
    Claim {
        name: "Tauri 2 + React/TypeScript desktop GUI",
        status: Status::Proposed,
        note: "approved product direction; the shipped interactive UI remains the ratatui TUI",
        instead: Some("abbey tui"),
    },
    Claim {
        name: "provider-neutral Abbey-owned agent and tool runtime / MCP-ACP host",
        status: Status::Proposed,
        note: "approved product direction; current execution is delegated to configured cursor-agent, abi, fm, or grok backends",
        instead: Some("abbey runtime · abbey mcp|acp · --approve-mcps"),
    },
    Claim {
        name: "production-capable local model weights",
        status: Status::Proposed,
        note: "approved product direction; Max/Gemma names currently remain role bindings, not bundled weights",
        instead: Some("ABBEY_BACKEND=fm · abbey weights"),
    },
    Claim {
        name: "fine-tuning / LoRA pipeline",
        status: Status::Proposed,
        note: "approved product direction; train_candidate remains curation substrate and performs no weight updates",
        instead: Some("abbey lora · abbey learn-review · abbey learn-stats"),
    },
    Claim {
        name: "GPU/NPU/TPU compilation, training, and inference in Abbey",
        status: Status::Proposed,
        note: "approved product direction; current compute commands detect and report hardware only",
        instead: Some("abbey accel · abbey platform compute"),
    },
    Claim {
        name: "local neural speech / image / video models",
        status: Status::Proposed,
        note: "approved product direction; current voice is platform I/O and current media generation is delegated to agent tools",
        instead: Some("abbey voice · abbey --image · abbey imagine"),
    },
    Claim {
        name: "personal-unrestricted separate edition",
        status: Status::Proposed,
        note: "approved only as an explicitly separate, locally controlled edition with isolation and auditable consent; shipped Abbey keeps allowlist + --confirm",
        instead: Some("abbey allowlist · abbey os execute --confirm · abbey shell"),
    },
    Claim {
        name: "authenticated local three-VM shared-compute proof, then production separate-host / geographic-HA / multi-GPU mesh",
        status: Status::Proposed,
        note: "approved product direction; the authenticated local multi-process proof does not establish separate-VM deployment",
        instead: Some("abbey mesh local-demo · abbey subagents --peers (same host)"),
    },
    // —— Blocked proof ——
    Claim {
        name: "self-hosted Linux CI execution proof",
        status: Status::Blocked,
        note: "ABI dependency blocker resolved by merged ABI 32e372d7f522f5a6c9c0ef92c5b9612b52cfea05; macOS ARM64 self-hosted is registered, but Linux ARM64 is not provisioned and its job stays gated by an explicit repository variable; Linux/Windows runtime proof remains open",
        instead: Some(
            "run ./check.sh locally; provision/register Linux ARM64 and obtain successful Linux/Windows jobs before claiming cross-platform CI green",
        ),
    },
    // —— Out of scope ——
    Claim {
        name: "unrestricted shell or allowlist bypass in the shipped edition",
        status: Status::OutOfScope,
        note: "the shipped edition keeps os_control allowlist + --confirm as a safety invariant; only the separately packaged personal edition is Proposed",
        instead: Some("abbey allowlist · abbey os execute --confirm · abbey shell"),
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
        name: "bundled cloud TTS/STT SaaS",
        status: Status::OutOfScope,
        note: "cloud speech services are not bundled; local neural speech is Proposed separately",
        instead: Some("abbey voice · System Settings → Spoken Content"),
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

/// Serialize the canonical claims ledger for repository synchronization tools.
pub fn manifest_json() -> Result<String> {
    let rows = CLAIMS
        .iter()
        .map(|claim| {
            serde_json::json!({
                "name": claim.name,
                "status": claim.status.key(),
                "note": claim.note,
                "instead": claim.instead,
            })
        })
        .collect::<Vec<_>>();
    Ok(serde_json::to_string_pretty(&rows)?)
}

/// Print the gate. `filter`: None = all sections; otherwise a status or keyword.
pub fn print_claims(filter: Option<&str>) -> Result<i32> {
    let filter = filter.map(str::trim).filter(|s| !s.is_empty());
    match filter.map(str::to_ascii_lowercase).as_deref() {
        None | Some("all") => {
            print_section(Status::Current);
            println!();
            print_section(Status::Partial);
            println!();
            print_section(Status::Proposed);
            println!();
            print_section(Status::Blocked);
            println!();
            print_section(Status::OutOfScope);
            print_footer();
        }
        Some("current" | "shipped") => print_section(Status::Current),
        Some("partial" | "part") => print_section(Status::Partial),
        Some("proposed" | "prop" | "roadmap") => {
            print_section(Status::Proposed);
            print_footer();
        }
        Some("oos" | "out" | "out-of-scope" | "deferred") => {
            print_section(Status::OutOfScope);
            print_footer();
        }
        Some("blocked" | "block") => {
            print_section(Status::Blocked);
            print_footer();
        }
        Some(key) => {
            let hits = lookup(key);
            if hits.is_empty() {
                bail!(
                    "no claims matching `{key}` — try: abbey claims current|partial|proposed|blocked|oos"
                );
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
        Status::Partial => "Partial (some shipped surface; stated gaps remain)",
        Status::Proposed => "Proposed (designed — not claimed live)",
        Status::Blocked => "Blocked (implementation or proof needs an external prerequisite)",
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
        Status::Partial => "~",
        Status::Proposed => "·",
        Status::Blocked => "!",
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
        "\nrefuse:  abbey claims refuse <lora|multinode|npu|gui|…>  (exit 2)\n\
         docs:    AGENTS.md claims gate · docs/architecture.md · docs/identity.md\n\
         rule:    Partial/Proposed/Blocked/OOS verbs must fail honestly — never silent success"
    );
}

/// Map an unavailable user verb to a non-Current claim and refuse with exit 2.
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
            "Fine-tuning / LoRA is Proposed but not implemented. train_candidate is curation only.",
        ),
        "multinode" | "multi-node" | "cluster" | "mesh" | "multi-gpu" | "distributed-mesh" => (
            "three-VM",
            "An authenticated local three-VM shared-compute proof is Proposed; production separate-physical-host, geographic-HA, and multi-GPU operation remains Proposed even after that proof. The same-host multi-process proof is Current on Unix hosts.",
        ),
        "npu" | "tpu" | "gpu" | "cuda" | "metal" | "ane" => (
            "GPU/NPU/TPU",
            "GPU/NPU/TPU compilation, training, and inference in Abbey are Proposed but not implemented. Host detect is Current.",
        ),
        "vision" | "vlm" | "video-weights" | "local-vision" => (
            "neural speech",
            "Local neural image/video models are Proposed but not implemented. Path attach + delegated agent-tool generation are Current.",
        ),
        "cot" | "chain-of-thought" | "cot-ui" | "cot-engine" => (
            "CoT",
            "Abbey-owned CoT engine/UI is Out of scope. Transcript viewer + Cursor thinking wrap are Current.",
        ),
        "weights" | "qwen" | "local-gemma" | "own-model" => (
            "local model",
            "Production-capable local weights are Proposed but not implemented. Max/Gemma remain role bindings.",
        ),
        "cost" | "tokens" | "billing" => (
            "cost",
            "Fake cost/token accounting is Out of scope. /cost is N/A.",
        ),
        "mcp-host" | "acp-host" | "host" | "tool-runtime" | "tool-host" => (
            "tool runtime",
            "An Abbey-owned provider-neutral tool runtime / MCP-ACP host is Proposed but not implemented. Inventory and peer launch are Current.",
        ),
        "shell" | "unrestricted" | "os-unrestricted" | "allowlist-bypass" | "yolo-shell" => (
            "personal-unrestricted",
            "A personal-unrestricted separate edition is Proposed but not implemented. The shipped edition keeps allowlist + --confirm and refuses bypass.",
        ),
        "accel" | "accelerator" | "accelerators" => (
            "GPU/NPU/TPU",
            "GPU/NPU/TPU compilation, training, and inference in Abbey are Proposed but not implemented. Host detect is Current.",
        ),
        "gui" | "window" | "windowed" | "desktop" | "tauri" | "react" => (
            "Tauri 2",
            "The Tauri 2 + React/TypeScript desktop GUI is Proposed but not implemented. The ratatui TUI is Current.",
        ),
        "speech" | "local-speech" | "image" | "video" | "neural-media" => (
            "neural speech",
            "Local neural speech/image/video models are Proposed but not implemented. Platform voice I/O and delegated media tools are Current.",
        ),
        other => {
            eprintln!(
                "abbey: unknown refuse topic `{other}`\n\
                 try: lora · multinode · npu · weights · shell · cost · mcp-host · gui"
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
                "abbey claims — Current / Partial / Proposed / Blocked / Out of scope gate\n\
                 \n\
                 usage:\n\
                   abbey claims              # full gate\n\
                   abbey claims partial      # partially shipped\n\
                   abbey claims proposed     # Proposed only\n\
                   abbey claims blocked      # external blockers\n\
                   abbey claims oos          # Out of scope only\n\
                   abbey claims current      # shipped\n\
                   abbey claims <keyword>    # search\n\
                   abbey claims refuse <topic>\n\
                 \n\
                 topics: lora · multinode · npu · weights · shell · cost · mcp-host · gui\n\
                 note: embeddings are Current; `claims refuse embeddings` reports that status"
            );
            return Ok(0);
        }
        return print_claims(None);
    }

    match args[0].as_str() {
        "manifest" => {
            output::println(manifest_json()?)?;
            Ok(0)
        }
        "refuse" | "no" | "deny" => {
            let topic = args.get(1).map(String::as_str).unwrap_or("");
            if topic.is_empty() {
                bail!("usage: abbey claims refuse <lora|multinode|npu|…>");
            }
            refuse(topic)
        }
        "proposed" | "prop" | "roadmap" => print_claims(Some("proposed")),
        "partial" | "part" => print_claims(Some("partial")),
        "blocked" | "block" => print_claims(Some("blocked")),
        "oos" | "out" | "out-of-scope" | "deferred" => print_claims(Some("oos")),
        "current" | "shipped" => print_claims(Some("current")),
        other => print_claims(Some(other)),
    }
}

pub fn status_line() -> String {
    let cur = by_status(Status::Current).count();
    let partial = by_status(Status::Partial).count();
    let prop = by_status(Status::Proposed).count();
    let blocked = by_status(Status::Blocked).count();
    let oos = by_status(Status::OutOfScope).count();
    format!(
        "claims:    {cur} Current · {partial} Partial · {prop} Proposed · {blocked} Blocked · {oos} Out of scope — `abbey claims`"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gate_has_all_five_statuses() {
        assert_eq!(by_status(Status::Current).count(), 23);
        assert_eq!(by_status(Status::Partial).count(), 1);
        assert_eq!(by_status(Status::Proposed).count(), 8);
        assert_eq!(by_status(Status::Blocked).count(), 1);
        assert_eq!(by_status(Status::OutOfScope).count(), 5);
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
    fn approved_expansion_is_proposed() {
        let hits = lookup("lora");
        assert!(hits.iter().any(|c| c.status == Status::Proposed));
        for keyword in [
            "Tauri 2",
            "tool runtime",
            "local model",
            "GPU/NPU/TPU",
            "neural speech",
            "personal-unrestricted",
            "three-VM",
        ] {
            assert!(
                lookup(keyword).iter().any(|c| c.status == Status::Proposed),
                "missing Proposed claim for {keyword}"
            );
        }
    }

    #[test]
    fn ci_proof_is_blocked_after_abi_dependency_resolution() {
        let claim = lookup("self-hosted")
            .into_iter()
            .find(|c| c.status == Status::Blocked)
            .expect("blocked self-hosted CI claim");
        assert!(
            claim
                .note
                .contains("32e372d7f522f5a6c9c0ef92c5b9612b52cfea05")
        );
        assert!(claim.note.contains("Linux ARM64"));
        assert!(claim.note.contains("repository variable"));
    }

    #[test]
    fn shipped_unrestricted_bypass_remains_out_of_scope() {
        let claim = lookup("allowlist bypass")
            .into_iter()
            .find(|c| c.status == Status::OutOfScope)
            .expect("shipped-edition bypass claim");
        assert!(claim.name.contains("shipped edition"));
        assert!(claim.note.contains("separately packaged"));
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
        assert_eq!(refuse("gui").unwrap(), 2);
    }

    #[test]
    fn lookup_shared_compute_roadmap() {
        let hits = lookup("three-VM");
        assert!(hits.iter().any(|c| c.status == Status::Proposed));
    }

    #[test]
    fn manifest_is_ordered_machine_readable_claims_source() {
        let manifest: Vec<serde_json::Value> =
            serde_json::from_str(&manifest_json().unwrap()).unwrap();
        assert_eq!(manifest.len(), CLAIMS.len());
        assert_eq!(manifest[0]["name"], CLAIMS[0].name);
        assert_eq!(manifest[0]["status"], CLAIMS[0].status.key());
        assert_eq!(
            manifest.last().unwrap()["name"],
            CLAIMS.last().unwrap().name
        );
    }
}
