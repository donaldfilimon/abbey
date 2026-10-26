//! Evidence failure belongs to one channel, not every proof of a capability.
use super::*;

fn channel_claim(status: Status) -> Claim {
    let mut claim = CLAIMS[0];
    claim.id = "fixture-channels";
    claim.status = status;
    claim.blocker_owner = None;
    claim.evidence.local_live = EvidenceState::Verified(&["local-proof.log"]);
    claim.evidence.external_required = EvidenceState::NotRequired("fixture");
    claim
}

#[test]
fn failed_external_channel_preserves_verified_local_proof() {
    let mut claim = channel_claim(Status::Failed);
    claim.evidence.external_required = EvidenceState::Failed(&["external-failure.log"]);
    validate_claims(&[claim]).unwrap();
    assert_eq!(
        evidence_state_json(claim.evidence.local_live)["state"],
        "verified"
    );
    assert_eq!(
        evidence_state_json(claim.evidence.external_required),
        serde_json::json!({"state":"failed","refs":["external-failure.log"]})
    );
}

#[test]
fn expired_external_channel_preserves_verified_local_proof() {
    let mut claim = channel_claim(Status::Expired);
    claim.evidence.external_required = EvidenceState::Expired(&["external-proof-expired.log"]);
    validate_claims(&[claim]).unwrap();
    assert_eq!(
        evidence_state_json(claim.evidence.local_live)["state"],
        "verified"
    );
    assert_eq!(
        evidence_state_json(claim.evidence.external_required)["state"],
        "expired"
    );
}

#[test]
fn current_claim_cannot_hide_failed_or_expired_evidence() {
    for evidence in [
        EvidenceState::Failed(&["failure.log"]),
        EvidenceState::Expired(&["expired.log"]),
    ] {
        let mut claim = channel_claim(Status::Current);
        claim.evidence.external_required = evidence;
        assert!(validate_claims(&[claim]).is_err());
        claim.status = Status::Partial;
        validate_claims(&[claim]).unwrap();
    }
}

#[test]
fn failed_and_expired_channels_require_nonempty_unique_references() {
    for state in [
        EvidenceState::Failed(&[]),
        EvidenceState::Expired(&[" "]),
        EvidenceState::Failed(&["same", "same"]),
    ] {
        assert!(validate_evidence_state("fixture", "external_required", state).is_err());
    }
}
