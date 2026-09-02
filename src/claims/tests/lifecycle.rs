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
    ok.instead = Some("fixture-replacement");
    let replacement = lifecycle_claim("fixture-replacement", Status::Current);
    validate_claims(&[ok, replacement])
        .expect("a superseded claim naming its replacement is valid");

    for replacement in ["missing-claim", "fixture-superseded"] {
        let mut invalid = lifecycle_claim("fixture-superseded", Status::Superseded);
        invalid.instead = Some(replacement);
        let err = validate_claims(&[invalid]).expect_err("replacement must resolve elsewhere");
        assert!(err.to_string().contains("different existing replacement"));
    }

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
            Status::Superseded => claim.instead = Some("fixture-replacement"),
            Status::Expired => {
                claim.evidence.local_live = EvidenceState::Required(&["re-run the gate"]);
            }
            _ => {}
        }
        let replacement = lifecycle_claim("fixture-replacement", Status::Current);
        let err = match validate_claims(&[claim, replacement]) {
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

#[test]
fn every_status_key_is_accepted_by_the_manifest_consumer() {
    // The Rust vocabulary and the Python sync tool's allowlist are two
    // separate lists that must agree. They drifted once already: the four
    // lifecycle states existed in Rust while tools/check_claims_sync.py
    // still had a five-way allowlist that RAISES on anything else, so the
    // gate stayed green only because no claim occupied a new state yet.
    //
    // This reads the `statuses = {...}` literal SPECIFICALLY rather than
    // searching the whole file. A whole-file `contains` is vacuous here:
    // every key also appears in two label dictionaries, so the naive
    // version of this test passed with the allowlist deliberately broken.
    let tool = std::fs::read_to_string("tools/check_claims_sync.py")
        .expect("sync tool must be readable from the crate root");
    let start = tool
        .find("statuses = {")
        .expect("sync tool must declare a `statuses` allowlist");
    let end = start
        + tool[start..]
            .find('}')
            .expect("`statuses` allowlist must be closed");
    let allowlist = &tool[start..end];

    for status in Status::every() {
        let quoted = format!("\"{}\"", status.key());
        assert!(
            allowlist.contains(&quoted),
            "check_claims_sync.py's status allowlist is missing {} — a claim \
             entering that state would raise instead of generating docs",
            status.key()
        );
    }
}

#[test]
fn the_manifest_declares_the_schema_that_matches_its_vocabulary() {
    // The manifest emits `status` per claim, so its value set is part of the
    // contract. Growing that set without moving the schema version would
    // leave generated docs asserting a schema they no longer describe.
    let manifest = manifest_json().expect("manifest must serialize");
    let parsed: serde_json::Value =
        serde_json::from_str(&manifest).expect("manifest must be valid JSON");
    assert_eq!(
        parsed["schema_version"], CLAIMS_SCHEMA_VERSION,
        "manifest must declare its own schema version"
    );
    assert_eq!(
        CLAIMS_SCHEMA_VERSION, 2,
        "schema 2 is the version that admits the lifecycle status vocabulary"
    );
}

#[test]
fn every_status_is_reachable_through_its_own_filter_aliases() {
    // The dispatcher is now a table lookup rather than one match arm per
    // status, which trades nine near-identical arms for one risk: a new
    // variant with no alias entry would silently become unfilterable and
    // fall through to the keyword search. This makes that a test failure.
    let mut seen: HashSet<&str> = HashSet::new();
    for status in Status::every() {
        let aliases = status.filter_aliases();
        assert!(
            !aliases.is_empty(),
            "{} has no filter alias, so `abbey claims <token>` cannot select it",
            status.label()
        );
        // Deliberately NOT asserting that the primary alias equals the wire
        // key: `out_of_scope` is keyed that way for machines but its CLI
        // token is the short `oos`, which the footer text and the original
        // per-status arms both used. Pinning them equal would encode an
        // invariant this CLI never had.
        for alias in aliases {
            assert_eq!(
                Status::from_filter(alias),
                Some(status),
                "alias `{alias}` does not resolve back to {}",
                status.label()
            );
            assert!(
                seen.insert(alias),
                "alias `{alias}` is claimed by two statuses"
            );
        }
    }
    // The help string is derived, so it must list every status exactly once.
    let help = Status::filter_help();
    assert_eq!(help.split('|').count(), 9);
}

#[test]
fn status_round_trips_through_its_wire_projection() {
    // Two `From` impls replaced three hand-written mappers. They must stay
    // inverses: a drift here would silently relabel claims on the daemon
    // surface rather than failing to compile.
    use crate::app_core::ClaimStatus;
    for status in Status::every() {
        let wire = ClaimStatus::from(status);
        assert_eq!(
            Status::from(wire),
            status,
            "{} does not survive the wire round trip",
            status.label()
        );
    }
}
