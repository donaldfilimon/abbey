//! Claims gate — Current / Partial / Proposed / Blocked / Out of scope.
//!
//! Single machine-readable source for honesty. Docs and `AGENTS.md` stay the
//! human table; this module powers `abbey claims` and refusal paths so approved
//! roadmap work, externally blocked proof, and deliberately excluded work are
//! never silently implied by a missing error.

use crate::output;
use anyhow::{Result, bail};
use std::collections::HashSet;

mod registry;

use registry::{Claim, EvidenceState};
pub const CLAIMS: &[Claim] = registry::CLAIMS;
pub const CLAIMS_SCHEMA_VERSION: u16 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Current,
    Partial,
    Proposed,
    Blocked,
    OutOfScope,
    /// Evidence was gathered for this claim and did not hold.
    Failed,
    /// The claim was withdrawn by decision rather than disproved.
    Revoked,
    /// A different claim now carries this capability; `instead` names it.
    Superseded,
    /// Evidence lapsed against the version or environment it was bound to.
    Expired,
}

impl Status {
    /// Positional states: every claim is in exactly one of these, and each
    /// must carry at least one claim.
    ///
    /// Kept in one place so tests can partition the registry by status
    /// without restating per-status claim counts. Two branches can each add
    /// a claim and each bump such a literal consistently, so the textual
    /// merge succeeds while the literal silently goes stale.
    pub const CLASSIFYING: [Self; 5] = [
        Self::Current,
        Self::Partial,
        Self::Proposed,
        Self::Blocked,
        Self::OutOfScope,
    ];

    /// Terminal lifecycle states (constitution decision 68).
    ///
    /// Deliberately separate from [`Self::CLASSIFYING`]: these are
    /// legitimately **empty** in a healthy registry, so the "every status
    /// carries work" invariant must not be applied to them. A vocabulary
    /// that only exists once something has already failed cannot be
    /// introduced at the moment of failure — it has to be first-class
    /// beforehand, which is what decision 68 requires.
    pub const LIFECYCLE: [Self; 4] = [Self::Failed, Self::Revoked, Self::Superseded, Self::Expired];

    /// The complete vocabulary, positional first. Registry partitioning uses
    /// this, so a claim that moves into a lifecycle state stays reachable
    /// through `by_status` rather than vanishing from every listing.
    ///
    /// Derived from the two constants above rather than restated, so a new
    /// variant cannot be added to one and forgotten in the other.
    pub fn every() -> impl Iterator<Item = Self> {
        Self::CLASSIFYING.into_iter().chain(Self::LIFECYCLE)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Current => "Current",
            Self::Partial => "Partial",
            Self::Proposed => "Proposed",
            Self::Blocked => "Blocked",
            Self::OutOfScope => "Out of scope",
            Self::Failed => "Failed",
            Self::Revoked => "Revoked",
            Self::Superseded => "Superseded",
            Self::Expired => "Expired",
        }
    }

    fn key(self) -> &'static str {
        match self {
            Self::Current => "current",
            Self::Partial => "partial",
            Self::Proposed => "proposed",
            Self::Blocked => "blocked",
            Self::OutOfScope => "out_of_scope",
            Self::Failed => "failed",
            Self::Revoked => "revoked",
            Self::Superseded => "superseded",
            Self::Expired => "expired",
        }
    }
}

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

/// Validate evidence completeness and status semantics before exposing the
/// registry to synchronization tooling.
pub fn validate_registry() -> Result<()> {
    validate_claims(CLAIMS)
}

fn validate_claims(claims: &[Claim]) -> Result<()> {
    let mut ids = HashSet::with_capacity(claims.len());
    for claim in claims {
        if claim.id.is_empty()
            || claim.id.starts_with('-')
            || claim.id.ends_with('-')
            || claim.id.contains("--")
            || !claim
                .id
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        {
            bail!(
                "claim `{}` has invalid stable id `{}`",
                claim.name,
                claim.id
            );
        }
        if !ids.insert(claim.id) {
            bail!("duplicate claim id `{}`", claim.id);
        }
        for (field, value) in [
            ("name", claim.name),
            ("note", claim.note),
            ("next_action", claim.next_action),
        ] {
            if value.trim().is_empty() {
                bail!("claim `{}` has empty {field}", claim.id);
            }
        }
        validate_refs(
            claim.id,
            "implementation_refs",
            claim.evidence.implementation_refs,
        )?;
        validate_refs(
            claim.id,
            "automated_test_refs",
            claim.evidence.automated_test_refs,
        )?;
        validate_evidence_state(claim.id, "local_live", claim.evidence.local_live)?;
        validate_evidence_state(
            claim.id,
            "external_required",
            claim.evidence.external_required,
        )?;

        let implementation_is_empty = claim.evidence.implementation_refs.is_empty();
        let tests_are_empty = claim.evidence.automated_test_refs.is_empty();
        match claim.status {
            Status::Current => {
                require_built_evidence(claim, implementation_is_empty, tests_are_empty)?;
                if is_required(claim.evidence.local_live)
                    || is_required(claim.evidence.external_required)
                {
                    bail!(
                        "Current claim `{}` cannot carry unsatisfied required evidence",
                        claim.id
                    );
                }
                require_no_blocker_owner(claim)?;
            }
            Status::Partial => {
                require_built_evidence(claim, implementation_is_empty, tests_are_empty)?;
                if !is_required(claim.evidence.local_live)
                    && !is_required(claim.evidence.external_required)
                {
                    bail!(
                        "Partial claim `{}` must identify its missing proof",
                        claim.id
                    );
                }
                require_no_blocker_owner(claim)?;
            }
            Status::Proposed => {
                if !implementation_is_empty || !tests_are_empty {
                    bail!(
                        "Proposed claim `{}` cannot present implementation or tests as shipped evidence",
                        claim.id
                    );
                }
                if !is_required(claim.evidence.local_live) {
                    bail!(
                        "Proposed claim `{}` must require future local implementation proof",
                        claim.id
                    );
                }
                require_no_blocker_owner(claim)?;
            }
            Status::Blocked => {
                require_built_evidence(claim, implementation_is_empty, tests_are_empty)?;
                if !is_required(claim.evidence.external_required) {
                    bail!(
                        "Blocked claim `{}` must identify required external evidence",
                        claim.id
                    );
                }
                if claim
                    .blocker_owner
                    .is_none_or(|owner| owner.trim().is_empty())
                {
                    bail!("Blocked claim `{}` must identify a blocker owner", claim.id);
                }
            }
            Status::Failed => {
                // Something was built, or there would be nothing to fail.
                require_built_evidence(claim, implementation_is_empty, tests_are_empty)?;
                // Decision 63: evidence never auto-promotes. A claim whose
                // proof failed must not still present that proof as verified.
                if matches!(claim.evidence.local_live, EvidenceState::Verified(_)) {
                    bail!(
                        "Failed claim `{}` cannot present verified local evidence",
                        claim.id
                    );
                }
                require_no_blocker_owner(claim)?;
            }
            Status::Revoked => {
                // Withdrawn by decision, so nobody is going to gather the
                // outstanding proof. Leaving evidence Required would imply
                // work that will never happen.
                if is_required(claim.evidence.local_live)
                    || is_required(claim.evidence.external_required)
                {
                    bail!(
                        "Revoked claim `{}` cannot leave evidence outstanding",
                        claim.id
                    );
                }
                require_no_blocker_owner(claim)?;
            }
            Status::Superseded => {
                // Decision 68 is only useful if a superseded claim says what
                // replaced it; otherwise the capability silently disappears.
                if claim.instead.is_none_or(|value| value.trim().is_empty()) {
                    bail!(
                        "Superseded claim `{}` must name the claim that replaced it",
                        claim.id
                    );
                }
                require_no_blocker_owner(claim)?;
            }
            Status::Expired => {
                // It held once (decision 62 binds evidence to an exact
                // version and environment), so implementation and tests
                // exist — but the binding lapsed and re-proof is owed.
                require_built_evidence(claim, implementation_is_empty, tests_are_empty)?;
                if !is_required(claim.evidence.local_live) {
                    bail!(
                        "Expired claim `{}` must require fresh local proof",
                        claim.id
                    );
                }
                require_no_blocker_owner(claim)?;
            }
            Status::OutOfScope => {
                require_built_evidence(claim, implementation_is_empty, tests_are_empty)?;
                if !matches!(claim.evidence.local_live, EvidenceState::NotRequired(_))
                    || !matches!(
                        claim.evidence.external_required,
                        EvidenceState::NotRequired(_)
                    )
                {
                    bail!(
                        "Out-of-scope claim `{}` cannot carry pending or success evidence",
                        claim.id
                    );
                }
                require_no_blocker_owner(claim)?;
            }
        }
    }
    Ok(())
}

fn validate_refs(claim_id: &str, field: &str, refs: &[&str]) -> Result<()> {
    if refs.iter().any(|value| value.trim().is_empty()) {
        bail!("claim `{claim_id}` has an empty {field} entry");
    }
    let unique = refs.iter().copied().collect::<HashSet<_>>();
    if unique.len() != refs.len() {
        bail!("claim `{claim_id}` has duplicate {field} entries");
    }
    Ok(())
}

fn validate_evidence_state(claim_id: &str, field: &str, state: EvidenceState) -> Result<()> {
    match state {
        EvidenceState::Verified(refs) | EvidenceState::Required(refs) => {
            if refs.is_empty() {
                bail!("claim `{claim_id}` has {field} state without references");
            }
            validate_refs(claim_id, field, refs)
        }
        EvidenceState::NotRequired(reason) => {
            if reason.trim().is_empty() {
                bail!("claim `{claim_id}` has {field} without a reason");
            }
            Ok(())
        }
    }
}

fn require_built_evidence(
    claim: &Claim,
    implementation_is_empty: bool,
    tests_are_empty: bool,
) -> Result<()> {
    if implementation_is_empty || tests_are_empty {
        bail!(
            "{} claim `{}` requires implementation and automated-test evidence",
            claim.status.label(),
            claim.id
        );
    }
    Ok(())
}

fn require_no_blocker_owner(claim: &Claim) -> Result<()> {
    if claim.blocker_owner.is_some() {
        bail!(
            "{} claim `{}` cannot carry a blocker owner",
            claim.status.label(),
            claim.id
        );
    }
    Ok(())
}

fn is_required(state: EvidenceState) -> bool {
    matches!(state, EvidenceState::Required(_))
}

fn evidence_state_json(state: EvidenceState) -> serde_json::Value {
    match state {
        EvidenceState::Verified(refs) => serde_json::json!({
            "state": "verified",
            "refs": refs,
        }),
        EvidenceState::Required(refs) => serde_json::json!({
            "state": "required",
            "refs": refs,
        }),
        EvidenceState::NotRequired(reason) => serde_json::json!({
            "state": "not_required",
            "reason": reason,
        }),
    }
}

/// Serialize the canonical claims ledger for repository synchronization tools.
pub fn manifest_json() -> Result<String> {
    validate_registry()?;
    let rows = CLAIMS
        .iter()
        .map(|claim| {
            serde_json::json!({
                "id": claim.id,
                "name": claim.name,
                "status": claim.status.key(),
                "note": claim.note,
                "instead": claim.instead,
                "evidence": {
                    "implementation_refs": claim.evidence.implementation_refs,
                    "automated_test_refs": claim.evidence.automated_test_refs,
                    "local_live": evidence_state_json(claim.evidence.local_live),
                    "external_required": evidence_state_json(claim.evidence.external_required),
                },
                "next_action": claim.next_action,
                "blocker_owner": claim.blocker_owner,
            })
        })
        .collect::<Vec<_>>();
    Ok(serde_json::to_string_pretty(&serde_json::json!({
        "schema_version": CLAIMS_SCHEMA_VERSION,
        "claims": rows,
    }))?)
}

/// Print the gate. `filter`: None = all sections; otherwise a status or keyword.
pub fn print_claims(filter: Option<&str>) -> Result<i32> {
    let filter = filter.map(str::trim).filter(|s| !s.is_empty());
    match filter.map(str::to_ascii_lowercase).as_deref() {
        None | Some("all") => {
            // Ordered by Status::every() so this listing and the tests that
            // partition the registry cannot disagree about which statuses
            // exist, and a new status appears here without a second edit.
            // Lifecycle sections are skipped while empty so a healthy
            // registry does not print four blank headings, but a claim that
            // moves into one becomes visible here without another edit.
            for (index, status) in Status::every().enumerate() {
                if Status::LIFECYCLE.contains(&status) && by_status(status).count() == 0 {
                    continue;
                }
                if index > 0 {
                    println!();
                }
                print_section(status);
            }
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
        Some("failed" | "fail") => {
            print_section(Status::Failed);
            print_footer();
        }
        Some("revoked" | "revoke") => {
            print_section(Status::Revoked);
            print_footer();
        }
        Some("superseded" | "supersede") => {
            print_section(Status::Superseded);
            print_footer();
        }
        Some("expired" | "expire") => {
            print_section(Status::Expired);
            print_footer();
        }
        Some(key) => {
            let hits = lookup(key);
            if hits.is_empty() {
                bail!(
                    "no claims matching `{key}` — try: abbey claims current|partial|proposed|blocked|oos|failed|revoked|superseded|expired"
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
        Status::Failed => "Failed (evidence was gathered and did not hold)",
        Status::Revoked => "Revoked (withdrawn by decision, not disproved)",
        Status::Superseded => "Superseded (another claim now carries it)",
        Status::Expired => "Expired (evidence lapsed against its version or environment)",
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
        Status::Failed => "✗",
        Status::Revoked => "✗",
        Status::Superseded => "→",
        Status::Expired => "⧖",
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

/// Claim-lookup key for the shipped-edition shell / allowlist-bypass refusal.
/// It must surface BOTH facts: the Proposed personal-unrestricted separate
/// edition AND the Out-of-scope shipped-edition bypass claim.
const SHELL_BYPASS_CLAIM_KEY: &str = "allowlist";

/// Non-Current claims a refusal surfaces for `claim_key`.
fn refusal_claims(claim_key: &str) -> Vec<&'static Claim> {
    lookup(claim_key)
        .into_iter()
        .filter(|c| c.status != Status::Current)
        .collect()
}

/// Map an unavailable user verb to a non-Current claim and refuse with exit 2.
const MCP_HOST_REFUSAL_DETAIL: &str = "An Abbey-owned provider-neutral tool runtime that \
    *consumes* external MCP/ACP servers is Proposed but not implemented. Current: config \
    inventory, peer launch, and `abbey mcp serve` — Abbey's own read-only MCP server over \
    stdio and unauthenticated loopback-only HTTP. Non-loopback HTTP, HTTPS/TLS, OAuth, and \
    consuming external servers remain unavailable.";

/// Accelerator refusal. Narrowed when kernel execution shipped: the Proposed
/// scope (compilation / training / inference) is unchanged and still refuses,
/// but the detail must name what *is* Current or the refusal overstates.
const ACCEL_REFUSAL_DETAIL: &str = "GPU/NPU/TPU compilation, training, and model inference in \
    Abbey are Proposed but not implemented, and device residency/placement is not observable \
    here. Current: host presence detect (`abbey platform compute`), and — behind \
    `--features accel` — real numerical kernel execution on Metal with CPU-oracle parity \
    (`abbey accel verify`). That verifies arithmetic, not compilation, training, inference, \
    placement, or speedup.";

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
        "npu" | "tpu" | "gpu" | "cuda" | "metal" | "ane" => ("GPU/NPU/TPU", ACCEL_REFUSAL_DETAIL),
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
        "mcp-host" | "acp-host" | "host" | "tool-runtime" | "tool-host" => {
            ("tool runtime", MCP_HOST_REFUSAL_DETAIL)
        }
        "shell" | "unrestricted" | "os-unrestricted" | "allowlist-bypass" | "yolo-shell" => (
            SHELL_BYPASS_CLAIM_KEY,
            "A personal-unrestricted separate edition is Proposed but not implemented, and allowlist bypass in the shipped edition is Out of scope. Shipped Abbey keeps allowlist + --confirm and refuses bypass.",
        ),
        "accel" | "accelerator" | "accelerators" => ("GPU/NPU/TPU", ACCEL_REFUSAL_DETAIL),
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

    let hits = refusal_claims(claim_key);
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
#[path = "claims/tests.rs"]
mod tests;
