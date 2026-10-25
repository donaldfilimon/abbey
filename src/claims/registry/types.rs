use super::super::Status;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvidenceState {
    Verified(&'static [&'static str]),
    Required(&'static [&'static str]),
    /// An observed failure bound to this channel; other channels keep their proof.
    #[allow(
        dead_code,
        reason = "schema 3 must admit a failed channel before the first failure"
    )]
    Failed(&'static [&'static str]),
    /// Previously gathered evidence whose version/environment binding lapsed.
    #[allow(
        dead_code,
        reason = "schema 3 must admit expired evidence before the first expiry"
    )]
    Expired(&'static [&'static str]),
    NotRequired(&'static str),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClaimEvidence {
    pub implementation_refs: &'static [&'static str],
    pub automated_test_refs: &'static [&'static str],
    pub local_live: EvidenceState,
    pub external_required: EvidenceState,
}

#[derive(Debug, Clone, Copy)]
pub struct Claim {
    /// Stable machine identifier. Never derive this from mutable display text.
    pub id: &'static str,
    pub name: &'static str,
    pub status: Status,
    /// Short note — evidence boundary, blocker, or Current caveat.
    pub note: &'static str,
    /// What to use instead (Current substitute), if any.
    pub instead: Option<&'static str>,
    pub evidence: ClaimEvidence,
    pub next_action: &'static str,
    /// The party able to remove an external blocker; only Blocked rows use it.
    pub blocker_owner: Option<&'static str>,
}
