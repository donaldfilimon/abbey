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
pub const CLAIMS_SCHEMA_VERSION: u16 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Current,
    Partial,
    Proposed,
    Blocked,
    OutOfScope,
}

impl Status {
    /// Every status the gate can classify a claim under.
    ///
    /// Kept in one place so tests can partition the registry by status
    /// without restating per-status claim counts. Two branches can each add
    /// a claim and each bump such a literal consistently, so the textual
    /// merge succeeds while the literal silently goes stale.
    pub const ALL: [Self; 5] = [
        Self::Current,
        Self::Partial,
        Self::Proposed,
        Self::Blocked,
        Self::OutOfScope,
    ];

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
            // Ordered by Status::ALL so this listing and the tests that
            // partition the registry cannot disagree about which statuses
            // exist, and a new status appears here without a second edit.
            for (index, status) in Status::ALL.iter().enumerate() {
                if index > 0 {
                    println!();
                }
                print_section(*status);
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
mod tests {
    use super::*;

    #[test]
    fn gate_has_all_five_statuses() {
        // Derived from the registry on purpose. Hardcoded per-status totals
        // are a semantic-merge hazard: two branches each add one claim and
        // each bump the literal to the same value, so git merges the text
        // cleanly and the assertion silently stops describing the registry.
        // Everything below stays true no matter how many claims exist, and
        // still fails for the cases the test name promises to catch.
        for (index, status) in Status::ALL.iter().enumerate() {
            assert!(
                !Status::ALL[..index].contains(status),
                "{} is listed twice in Status::ALL",
                status.label()
            );
            assert!(
                by_status(*status).count() > 0,
                "no claim carries status {}",
                status.label()
            );
        }

        // An exact partition. This fails if a claim is unreachable through
        // by_status — which is what a new Status variant missing from
        // Status::ALL looks like — or if by_status ever double-counts.
        let classified: usize = Status::ALL
            .iter()
            .map(|status| by_status(*status).count())
            .sum();
        assert_eq!(
            classified,
            CLAIMS.len(),
            "every claim must be reachable through exactly one Status::ALL entry"
        );

        // The registry must also be well-formed, not merely countable.
        validate_registry().expect("canonical registry must be internally valid");
    }

    #[test]
    fn stable_ids_are_unique_and_well_formed() {
        let ids = CLAIMS.iter().map(|claim| claim.id).collect::<HashSet<_>>();
        assert_eq!(ids.len(), CLAIMS.len());
        for id in ids {
            assert!(!id.is_empty());
            assert!(!id.starts_with('-'));
            assert!(!id.ends_with('-'));
            assert!(!id.contains("--"));
            assert!(
                id.bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-'),
                "invalid claim id: {id}"
            );
        }
    }

    #[test]
    fn every_claim_has_exact_structured_evidence_coverage() {
        validate_registry().expect("canonical registry must be internally valid");
        for claim in CLAIMS {
            assert!(!claim.next_action.trim().is_empty(), "{}", claim.id);
            validate_evidence_state(claim.id, "local_live", claim.evidence.local_live).unwrap();
            validate_evidence_state(
                claim.id,
                "external_required",
                claim.evidence.external_required,
            )
            .unwrap();
            if claim.status == Status::Proposed {
                assert!(
                    claim.evidence.implementation_refs.is_empty(),
                    "{}",
                    claim.id
                );
                assert!(
                    claim.evidence.automated_test_refs.is_empty(),
                    "{}",
                    claim.id
                );
            } else {
                assert!(
                    !claim.evidence.implementation_refs.is_empty(),
                    "{}",
                    claim.id
                );
                assert!(
                    !claim.evidence.automated_test_refs.is_empty(),
                    "{}",
                    claim.id
                );
            }
        }
    }

    #[test]
    fn registry_status_invariants_fail_closed() {
        let mut claims = CLAIMS.to_vec();

        let current = claims
            .iter()
            .position(|claim| claim.status == Status::Current)
            .unwrap();
        claims[current].evidence.local_live = EvidenceState::Required(&["missing proof"]);
        assert!(validate_claims(&claims).is_err());

        claims = CLAIMS.to_vec();
        let partial = claims
            .iter()
            .position(|claim| claim.status == Status::Partial)
            .unwrap();
        claims[partial].evidence.local_live = EvidenceState::NotRequired("none");
        assert!(validate_claims(&claims).is_err());

        claims = CLAIMS.to_vec();
        let proposed = claims
            .iter()
            .position(|claim| claim.status == Status::Proposed)
            .unwrap();
        claims[proposed].evidence.implementation_refs = &["src/not-shipped.rs"];
        assert!(validate_claims(&claims).is_err());

        claims = CLAIMS.to_vec();
        let blocked = claims
            .iter()
            .position(|claim| claim.status == Status::Blocked)
            .unwrap();
        claims[blocked].blocker_owner = None;
        assert!(validate_claims(&claims).is_err());

        claims = CLAIMS.to_vec();
        let out_of_scope = claims
            .iter()
            .position(|claim| claim.status == Status::OutOfScope)
            .unwrap();
        claims[out_of_scope].evidence.external_required =
            EvidenceState::Required(&["impossible proof"]);
        assert!(validate_claims(&claims).is_err());
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
    fn app_core_daemon_claim_stays_narrower_than_owned_runtime() {
        let daemon = CLAIMS
            .iter()
            .find(|claim| claim.name.starts_with("shared application core"))
            .expect("app-core daemon claim");
        assert_eq!(daemon.status, Status::Current);
        assert!(daemon.note.contains("abbey daemon status|claims"));
        assert!(daemon.note.contains("protocol-v1"));
        assert!(daemon.note.contains("read-only"));
        assert!(daemon.note.contains("separate protocol-v2 surface"));
        assert!(daemon.note.contains("provider-neutral"));
        assert!(daemon.note.contains("remain unavailable"));

        let bounded_runs = CLAIMS
            .iter()
            .find(|claim| claim.id == "daemon-protocol-v2-bounded-runs")
            .expect("bounded protocol-v2 run claim");
        assert_eq!(bounded_runs.status, Status::Current);
        assert!(bounded_runs.note.contains("startup-bound"));
        assert!(bounded_runs.note.contains("CLI and TUI slash surface"));
        assert!(bounded_runs.note.contains("frontend-neutral"));
        assert!(
            bounded_runs
                .note
                .contains("Requests cannot select executables")
        );
        for excluded in [
            "live subscriptions",
            "daemon-owned memory",
            "desktop bridge",
            "provider-neutral model ownership",
        ] {
            assert!(bounded_runs.note.contains(excluded));
        }
        assert!(
            bounded_runs
                .note
                .contains("Windows named pipes/Job Objects")
        );

        let owned_runtime = CLAIMS
            .iter()
            .find(|claim| claim.name.starts_with("provider-neutral Abbey-owned"))
            .expect("owned runtime claim");
        assert_eq!(owned_runtime.status, Status::Proposed);
    }

    #[test]
    fn legacy_conversation_migration_claim_stays_metadata_only() {
        let claim = CLAIMS
            .iter()
            .find(|claim| claim.id == "runtime-legacy-conversation-metadata-migration")
            .expect("legacy conversation migration claim");
        assert_eq!(claim.status, Status::Current);
        for boundary in [
            "canonical edition-scoped",
            "retained owner-only",
            "No transcript",
            "semantic-memory",
            "backend/title/run inference",
            "protocol/UI authority",
        ] {
            assert!(
                claim.note.contains(boundary),
                "missing boundary: {boundary}"
            );
        }
    }

    #[test]
    fn conversation_identity_mutation_claim_stays_tombstone_bounded_and_unix_local() {
        let claim = CLAIMS
            .iter()
            .find(|claim| claim.id == "runtime-conversation-identity-write-cutover")
            .expect("canonical identity write claim");
        assert_eq!(claim.status, Status::Current);
        for boundary in [
            "identity saves",
            "scope/all clears",
            "explicit tombstones",
            "owner-only fs4-locked recovery journal",
            "prepared-but-uncommitted",
            "prevents stale identity resurrection",
            "global fallback",
            "Aliases and conversation provenance, history, transcripts, semantic memory",
            "Reads still resolve through compatibility mirrors",
            "backend/title/run inference",
            "protocol/UI/MCP surfaces",
            "Windows runtime authority",
        ] {
            assert!(
                claim.note.contains(boundary),
                "missing boundary: {boundary}"
            );
        }
        assert!(claim.next_action.contains("`read_chat_for`"));
        assert!(
            claim
                .next_action
                .contains("digest-verified compatibility-mirror resolution")
        );
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
    fn shell_refusal_surfaces_proposed_edition_and_oos_shipped_bypass() {
        // The `shell`/`allowlist-bypass` refuse arm must print BOTH ledger
        // facts: the Proposed personal-unrestricted separate edition and the
        // Out-of-scope allowlist bypass in the shipped edition.
        let hits = refusal_claims(SHELL_BYPASS_CLAIM_KEY);
        assert!(
            hits.iter()
                .any(|c| c.status == Status::Proposed && c.name.contains("personal-unrestricted")),
            "missing Proposed personal-unrestricted separate-edition claim"
        );
        assert!(
            hits.iter()
                .any(|c| c.status == Status::OutOfScope && c.name.contains("shipped edition")),
            "missing Out-of-scope shipped-edition allowlist-bypass claim"
        );
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
    fn host_refusal_preserves_the_current_loopback_http_boundary() {
        assert!(MCP_HOST_REFUSAL_DETAIL.contains("loopback-only HTTP"));
        assert!(MCP_HOST_REFUSAL_DETAIL.contains("Non-loopback HTTP"));
        assert!(MCP_HOST_REFUSAL_DETAIL.contains("OAuth"));
        assert!(!MCP_HOST_REFUSAL_DETAIL.contains("stdio only"));
    }

    #[test]
    fn lookup_shared_compute_roadmap() {
        let hits = lookup("three-VM");
        assert!(hits.iter().any(|c| c.status == Status::Proposed));
    }

    #[test]
    fn manifest_is_ordered_machine_readable_claims_source() {
        let manifest: serde_json::Value = serde_json::from_str(&manifest_json().unwrap()).unwrap();
        assert_eq!(manifest["schema_version"], CLAIMS_SCHEMA_VERSION);
        let rows = manifest["claims"].as_array().expect("claims array");
        assert_eq!(rows.len(), CLAIMS.len());
        assert_eq!(rows[0]["id"], CLAIMS[0].id);
        assert_eq!(rows[0]["name"], CLAIMS[0].name);
        assert_eq!(rows[0]["status"], CLAIMS[0].status.key());
        assert!(rows[0]["evidence"]["implementation_refs"].is_array());
        assert!(rows[0]["evidence"]["automated_test_refs"].is_array());
        assert!(rows[0]["evidence"]["local_live"]["state"].is_string());
        assert!(rows[0]["evidence"]["external_required"]["state"].is_string());
        assert!(rows[0]["next_action"].is_string());
        assert!(rows[0].get("blocker_owner").is_some());
        assert_eq!(rows.last().unwrap()["name"], CLAIMS.last().unwrap().name);
    }
}
