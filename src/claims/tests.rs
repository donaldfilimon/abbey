use super::*;

#[test]
fn every_classifying_status_is_unique_and_carries_work() {
    // Derived from the registry on purpose. Hardcoded per-status totals
    // are a semantic-merge hazard: two branches each add one claim and
    // each bump the literal to the same value, so git merges the text
    // cleanly and the assertion silently stops describing the registry.
    // Everything below stays true no matter how many claims exist, and
    // still fails for the cases the test name promises to catch.
    for (index, status) in Status::CLASSIFYING.iter().enumerate() {
        assert!(
            !Status::CLASSIFYING[..index].contains(status),
            "{} is listed twice in Status::CLASSIFYING",
            status.label()
        );
        assert!(
            by_status(*status).count() > 0,
            "no claim carries status {}",
            status.label()
        );
    }

    // An exact partition over the WHOLE vocabulary, not just the positional
    // states. Summing CLASSIFYING alone would still equal CLAIMS.len() while
    // a superseded claim sat unreachable, so the partition has to be taken
    // over Status::every() for this assertion to keep meaning what it says.
    let classified: usize = Status::every()
        .map(|status| by_status(status).count())
        .sum();
    assert_eq!(
        classified,
        CLAIMS.len(),
        "every claim must be reachable through exactly one status"
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
        "claimed separately by daemon-protocol-v3-exact-model-inference",
        "real external HTTPS download evidence",
    ] {
        assert!(claim.note.contains(required), "missing `{required}`");
    }
}

#[test]
fn exact_model_inference_claim_is_fixture_bounded_and_surface_narrow() {
    let claim = CLAIMS
        .iter()
        .find(|claim| claim.id == "daemon-protocol-v3-exact-model-inference")
        .expect("exact-model inference claim");
    assert_eq!(claim.status, Status::Current);
    for boundary in [
        "separately negotiates infer_models",
        "already-loaded exact model ID and immutable revision",
        "domain-separated SHA-256",
        "1..=256 output tokens",
        "32 KiB",
        "positive native-operation evidence",
        "false no-fallback/no-mixed-execution evidence",
        "generated abi-bigram-v1 CPU-fixture-only",
        "synchronous, non-durable",
        "owner-only Unix",
        "No CLI, desktop, MCP, Windows, remote, protocol-v2, default route",
        "production weights, quality, performance, or residency claim",
    ] {
        assert!(claim.note.contains(boundary), "missing `{boundary}`");
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
fn ci_proof_stays_blocked_without_linux_and_windows_execution() {
    let claim = lookup("self-hosted")
        .into_iter()
        .find(|c| c.status == Status::Blocked)
        .expect("blocked self-hosted CI claim");
    assert!(claim.note.contains("public WDBX"));
    assert!(claim.note.contains("macOS ARM64"));
    assert!(claim.note.contains("Linux ARM64"));
    assert!(claim.note.contains("Windows"));
    assert!(claim.next_action.contains("Linux ARM64"));
}

#[test]
fn program_1_host_claim_is_current_only_at_data_contract_level_c1() {
    let claim = CLAIMS
        .iter()
        .find(|claim| claim.id == "program-1-abbey-contracts-host")
        .expect("Program 1 host claim");
    assert_eq!(claim.status, Status::Current);
    for boundary in [
        "C1",
        "data-only",
        "72e241e34967df318376bf68f4a0e2db13f5ebf17d1a219709731f1f470dbe8e",
        "no production federation authority",
        "no participant-consented live Discord evidence",
    ] {
        assert!(
            claim.note.contains(boundary),
            "missing boundary `{boundary}`"
        );
    }
    assert!(
        claim
            .evidence
            .implementation_refs
            .contains(&"src/abbey_contracts.rs")
    );
    assert!(
        claim
            .evidence
            .automated_test_refs
            .contains(&"tests/abbey_contracts.rs")
    );
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

// --- Terminal lifecycle states (constitution decision 68) -----------------
//
// These states are legitimately EMPTY in a healthy registry, so they cannot
// be proved against CLAIMS the way the positional states are. Synthetic
// claims are the only way to pin their validation semantics, and pinning
// them matters: a vocabulary that is only exercised once something has
// already failed is a vocabulary nobody has tested.

use super::registry::ClaimEvidence;

fn lifecycle_claim(id: &'static str, status: Status) -> Claim {
    Claim {
        id,
        name: "synthetic lifecycle fixture",
        status,
        note: "fixture used only to pin lifecycle validation semantics",
        instead: None,
        evidence: ClaimEvidence {
            implementation_refs: &["src/claims.rs"],
            automated_test_refs: &["src/claims/tests.rs"],
            local_live: EvidenceState::NotRequired("fixture"),
            external_required: EvidenceState::NotRequired("fixture"),
        },
        next_action: "fixture",
        blocker_owner: None,
    }
}

#[test]
fn lifecycle_states_are_separate_from_the_positional_partition() {
    // The "every status carries work" invariant must NOT reach the lifecycle
    // states, or an honest registry with nothing failed cannot validate.
    for status in Status::LIFECYCLE {
        assert!(
            !Status::CLASSIFYING.contains(&status),
            "{} must not be a positional state",
            status.label()
        );
        assert_eq!(
            by_status(status).count(),
            0,
            "{} should be empty in a healthy registry",
            status.label()
        );
    }
    // ...yet every one of them must still be reachable as a status.
    let every: Vec<_> = Status::every().collect();
    assert_eq!(every.len(), 9);
    for status in Status::LIFECYCLE {
        assert!(every.contains(&status));
    }
}

#[test]
fn every_status_has_a_distinct_label_and_key() {
    let labels: HashSet<_> = Status::every().map(Status::label).collect();
    let keys: HashSet<_> = Status::every().map(Status::key).collect();
    assert_eq!(labels.len(), 9, "labels must be distinct");
    assert_eq!(keys.len(), 9, "wire keys must be distinct");
}

#[test]
fn a_failed_claim_cannot_still_present_verified_local_evidence() {
    // Decision 63: evidence never auto-promotes. The inverse matters just as
    // much — evidence that stopped holding must not keep presenting itself.
    let mut claim = lifecycle_claim("fixture-failed", Status::Failed);
    claim.evidence.local_live = EvidenceState::Verified(&["src/claims.rs"]);
    let err = validate_claims(&[claim]).expect_err("verified proof under Failed must be rejected");
    assert!(
        err.to_string()
            .contains("cannot present verified local evidence")
    );

    // The honest shape passes.
    let ok = lifecycle_claim("fixture-failed", Status::Failed);
    validate_claims(&[ok]).expect("a failed claim with unverified evidence is valid");
}

#[test]
fn a_failed_claim_still_requires_the_evidence_that_failed() {
    let mut claim = lifecycle_claim("fixture-failed-bare", Status::Failed);
    claim.evidence.implementation_refs = &[];
    let err = validate_claims(&[claim]).expect_err("nothing was built, so nothing could fail");
    assert!(
        err.to_string()
            .contains("requires implementation and automated-test evidence")
    );
}

#[test]
fn a_revoked_claim_cannot_leave_evidence_outstanding() {
    // Withdrawn by decision means nobody is coming back for the proof;
    // leaving it Required advertises work that will never happen.
    let mut claim = lifecycle_claim("fixture-revoked", Status::Revoked);
    claim.evidence.local_live = EvidenceState::Required(&["run the acceptance matrix"]);
    let err = validate_claims(&[claim]).expect_err("outstanding proof under Revoked must fail");
    assert!(
        err.to_string()
            .contains("cannot leave evidence outstanding")
    );

    let ok = lifecycle_claim("fixture-revoked", Status::Revoked);
    validate_claims(&[ok]).expect("a revoked claim with settled evidence is valid");
}

#[test]
fn a_superseded_claim_must_name_its_replacement() {
    // Without this the capability silently disappears from the ledger, which
    // is exactly the failure mode the claims gate exists to prevent.
    let claim = lifecycle_claim("fixture-superseded", Status::Superseded);
    assert!(claim.instead.is_none());
    let err = validate_claims(&[claim]).expect_err("an unnamed replacement must be rejected");
    assert!(
        err.to_string()
            .contains("must name the claim that replaced it")
    );

    let mut ok = lifecycle_claim("fixture-superseded", Status::Superseded);
    ok.instead = Some("backend-cursor-agent");
    validate_claims(&[ok]).expect("a superseded claim naming its replacement is valid");

    // Whitespace is not a name.
    let mut blank = lifecycle_claim("fixture-superseded", Status::Superseded);
    blank.instead = Some("   ");
    validate_claims(&[blank]).expect_err("a blank replacement must be rejected");
}

#[test]
fn an_expired_claim_must_require_fresh_proof() {
    // Decision 62 binds evidence to an exact version and environment. When
    // that binding lapses the claim owes re-proof, not silence.
    let claim = lifecycle_claim("fixture-expired", Status::Expired);
    let err = validate_claims(&[claim]).expect_err("expired without re-proof must be rejected");
    assert!(err.to_string().contains("must require fresh local proof"));

    let mut ok = lifecycle_claim("fixture-expired", Status::Expired);
    ok.evidence.local_live = EvidenceState::Required(&["re-run the gate on the current toolchain"]);
    validate_claims(&[ok]).expect("an expired claim requiring re-proof is valid");
}

#[test]
fn no_lifecycle_state_may_carry_a_blocker_owner() {
    // blocker_owner means "an external party can unblock this". None of the
    // terminal states are waiting on anyone, so carrying an owner would
    // misreport a closed claim as actionable.
    for status in Status::LIFECYCLE {
        let mut claim = lifecycle_claim("fixture-owner", status);
        claim.blocker_owner = Some("Donald");
        match status {
            Status::Superseded => claim.instead = Some("backend-cursor-agent"),
            Status::Expired => {
                claim.evidence.local_live = EvidenceState::Required(&["re-run the gate"]);
            }
            _ => {}
        }
        let err = match validate_claims(&[claim]) {
            Ok(()) => panic!("{} must reject a blocker owner", status.label()),
            Err(err) => err.to_string(),
        };
        assert!(
            err.contains("cannot carry a blocker owner"),
            "{} accepted a blocker owner: {err}",
            status.label()
        );
    }
}
