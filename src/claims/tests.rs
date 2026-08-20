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
    assert!(daemon.note.contains("protocol-v3 envelope"));
    assert!(
        daemon
            .note
            .contains("canonical safe read-only tool inventory")
    );
    assert!(daemon.note.contains("exact canonical stable-claim reads"));
    assert!(
        daemon
            .note
            .contains("conditional bounded ABI-local model inventory")
    );
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
        "model registry lifecycle",
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
fn protocol_v3_safe_inventory_keeps_mcp_read_only_and_approval_lifecycle_narrow() {
    let claim = CLAIMS
        .iter()
        .find(|claim| claim.id == "daemon-protocol-v3-abi-model-inventory")
        .expect("protocol-v3 model inventory claim");
    assert_eq!(claim.status, Status::Current);
    for required in [
        "read_models",
        "list_tools",
        "canonical SAFE_TOOLS registry",
        "structurally read-only",
        "MCP remain exactly three read-only tools",
        "abbey_memory_mark_obsolete",
        "RequireConfirmation",
        "domain-separated SHA-256",
        "stores no raw input",
        "performs no effect",
        "Only an identical explicit InvokeTool resubmission",
        "atomically changes that approval to consumed",
        "startup-selected edition memory backend",
        "marks the one record obsolete without deleting provenance",
        "immediately after prepare and immediately after the effect",
        "requires a fresh call and approval",
        "neither downgrade nor automatic mutation replay exists",
        "negotiates decide_tool_approvals",
        "personal catalog and MCP remain exactly three read-only tools and do not receive mutation, decision, cancellation, or execution authority",
        "globally single-use decision ID",
        "fail closed",
        "cancel_tools",
        "globally single-use cancellation ID",
        "pending or approved records",
        "read_claims_by_id",
        "exact stable-ID",
        "same startup-owned ABI ModelProvider route used by protocol-v2 execution",
        "Missing or forged grants",
        "There is no fuzzy claim search",
        "negotiates read_memory",
        "summary-only search",
        "payload, provenance, source metadata, and raw IDs never cross the socket",
        "MCP receives no memory surface",
        "EffectScopedPolicy",
        "digest-only authorization",
        "single-use",
        "32 KiB",
        "general raw memory authority",
    ] {
        assert!(claim.note.contains(required), "missing `{required}`");
    }
}

#[test]
fn protocol_v3_model_lifecycle_claim_keeps_selection_startup_owned_and_evidence_narrow() {
    let claim = CLAIMS
        .iter()
        .find(|claim| claim.id == "daemon-protocol-v3-model-lifecycle")
        .expect("protocol-v3 model lifecycle claim");
    assert_eq!(claim.status, Status::Current);
    for required in [
        "owner-only startup document",
        "Ed25519 publisher-signed",
        "license-acceptance ledger",
        "external storage",
        "without fallback",
        "download_models",
        "manage_models",
        "globally single-use operation ID",
        "requests cannot select paths, URLs, trust keys, principals, devices, executables, environment, or a substitute model",
        "resumable hash verification and atomic publication",
        "Schema v7",
        "4,096 durable operations",
        "reopens as failed",
        "loaded-with-no-inference-evidence",
        "no registry URL, storage path, principal",
        "No prompt-inference command",
        "real external HTTPS download evidence",
    ] {
        assert!(claim.note.contains(required), "missing `{required}`");
    }
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
fn conversation_identity_claim_includes_canonical_reads_and_stays_unix_local() {
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
        "select cwd before global",
        "continue past cwd tombstones",
        "matches the canonical digest",
        "corrupt selected cwd mirror never falls back",
        "Provider create, resume, retry, and capture paths use the fallible resolver",
        "lossy wrapper is presentation-only",
        "Aliases and conversation provenance, history, transcripts, semantic memory",
        "Backend/title/run inference",
        "protocol/UI/MCP surfaces",
        "Windows runtime authority",
    ] {
        assert!(
            claim.note.contains(boundary),
            "missing boundary: {boundary}"
        );
    }
    assert!(
        claim
            .next_action
            .contains("Decompose every production Rust module")
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
            .contains("88c02fb550169e4cdb5e1df2bf6d1d13532e0e49")
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
