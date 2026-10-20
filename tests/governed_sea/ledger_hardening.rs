//! Scratch composition checks; admission and signatures remain WDBX-owned.

use abi_sea::composition::{PersistenceOutcome, propose_outcome};
use abi_wdbx::v3::episode::{
    ActorKind, ActorRef, EpisodeEvent, EpisodeSigner, EpisodeSource, EpisodeStore, EpisodeWrite,
    EvidenceLevel, GuildEpisodePolicy, SignatureStatus, StorePolicy, TerminalReason,
    TerminalStatus,
};
use std::collections::BTreeMap;
use std::path::PathBuf;

struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn signer() -> EpisodeSigner {
    EpisodeSigner::new(ed25519_dalek::SigningKey::from_bytes(&[31; 32]))
}

fn actor(principal: &str, kind: ActorKind) -> ActorRef {
    ActorRef {
        principal_id: principal.into(),
        kind,
    }
}

fn policy() -> StorePolicy {
    StorePolicy {
        contract_revision: 2,
        contract_digest: [9; 32],
        guilds: BTreeMap::from([(
            "guild_ref".into(),
            GuildEpisodePolicy {
                learning_enabled: true,
                policy_version: "v1".into(),
                token_budget: 100,
                storage_budget_bytes: 1_000_000,
                current_consent_epoch: Some(7),
            },
        )]),
    }
}

fn write(index: usize, event: EpisodeEvent) -> EpisodeWrite {
    EpisodeWrite {
        request_id: format!("request_{index}"),
        operation_id: "operation".into(),
        contract_revision: 2,
        contract_digest: [9; 32],
        guild_ref: "guild_ref".into(),
        consent_epoch: Some(7),
        source_type: EpisodeSource::DiscordVoice,
        policy_version: "v1".into(),
        evidence_level: EvidenceLevel::C2,
        event,
        token_cost: 1,
        expected_commitment: None,
        quiet: false,
    }
}

fn failed_restore_prefix(store: &mut EpisodeStore) {
    let events = [
        EpisodeEvent::Proposal {
            requested_by: actor("requester", ActorKind::HumanSubject),
            proposed_by: actor("service", ActorKind::Service),
        },
        EpisodeEvent::Approval {
            approved_by: actor("admin", ActorKind::GuildAdministrator),
        },
        EpisodeEvent::Execution {
            executed_by: actor("service", ActorKind::Service),
            voice: None,
        },
        EpisodeEvent::Compensation {
            compensated_by: actor("service", ActorKind::Service),
            exact_restore_observed: false,
        },
    ];
    for (index, event) in events.into_iter().enumerate() {
        assert_eq!(
            propose_outcome(store, &write(index, event), true),
            PersistenceOutcome::Committed
        );
    }
}

#[test]
fn failed_restore_refuses_success_without_changing_signed_evidence() {
    let scratch =
        Scratch(std::env::temp_dir().join(format!("abbey-sea-hardening-{}", uuid::Uuid::new_v4())));
    let mut store = EpisodeStore::open_with_signer(&scratch.0, policy(), signer()).unwrap();
    failed_restore_prefix(&mut store);
    let ledger = scratch.0.join("episodes.v1.jsonl");
    let bytes = std::fs::read(&ledger).unwrap();
    let receipts = store.retrieve("guild_ref", 10).unwrap();
    let usage = store.guild_usage("guild_ref");

    for reopen in [false, true] {
        if reopen {
            drop(store);
            store = EpisodeStore::open_with_signer(&scratch.0, policy(), signer()).unwrap();
        }
        for status in [TerminalStatus::Compensated, TerminalStatus::Completed] {
            let terminal = write(
                4,
                EpisodeEvent::Terminal {
                    status,
                    reason: TerminalReason::Completed,
                },
            );
            assert_eq!(
                propose_outcome(&mut store, &terminal, true),
                PersistenceOutcome::Refused
            );
            assert_eq!(std::fs::read(&ledger).unwrap(), bytes);
            assert_eq!(store.retrieve("guild_ref", 10).unwrap(), receipts);
            assert_eq!(store.guild_usage("guild_ref"), usage);
        }
        for receipt in &receipts {
            assert!(matches!(
                store
                    .signature_status("guild_ref", &receipt.episode_digest, |_| {
                        Some(signer().verifying_key())
                    })
                    .unwrap(),
                Some(SignatureStatus::Valid(_))
            ));
        }
    }
    let terminal = write(
        4,
        EpisodeEvent::Terminal {
            status: TerminalStatus::Failed,
            reason: TerminalReason::Completed,
        },
    );
    assert_eq!(
        propose_outcome(&mut store, &terminal, true),
        PersistenceOutcome::Committed
    );
    drop(store);
    let store = EpisodeStore::open_with_signer(&scratch.0, policy(), signer()).unwrap();
    assert_eq!(
        store
            .retrieve("guild_ref", 10)
            .unwrap()
            .last()
            .unwrap()
            .terminal_status,
        Some(TerminalStatus::Failed)
    );
}
