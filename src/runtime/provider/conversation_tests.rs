//! Durable stateless context must stay bound to the exact admitted turn.
use super::*;
use crate::app_core::{ConversationId, IdempotencyKey, RunState};
use crate::runtime::NewRunEvent;
use abi_agent_runtime::{Flow, RunContext, RuntimeError};
use std::sync::Mutex;

#[derive(Default)]
struct Probe {
    prompts: Mutex<Vec<String>>,
}
impl ModelProvider for Probe {
    fn id(&self) -> &str {
        "context-fixture"
    }
    fn run(&self, request: &ModelRequest, run: &mut RunContext<'_>) -> Result<(), RuntimeError> {
        self.prompts
            .lock()
            .unwrap()
            .push(request.messages()[0].content.clone());
        if run.emit(&ModelEvent::started(request.model())) == Flow::Stop {
            return Ok(());
        }
        run.emit(&ModelEvent::text(format!(
            "{}{}",
            "a".repeat(16_383),
            "🙂".repeat(100)
        )));
        Ok(())
    }
}
struct Fixture {
    root: std::path::PathBuf,
    store: RuntimeStore,
    probe: Arc<Probe>,
    executor: ModelProviderExecutor,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "abbey-v4-context-provider-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let store = RuntimeStore::open(&RuntimeStore::path_for_state_dir(&root)).unwrap();
        let probe = Arc::new(Probe::default());
        let routes = [
            (BackendSelection::Abi, "fixture"),
            (BackendSelection::FoundationModels, "system"),
        ]
        .map(|(backend, model)| {
            ProviderRoute::new(
                backend,
                model,
                probe.clone(),
                RunBudget::unlimited()
                    .with_max_events(16)
                    .with_max_output_tokens(32)
                    .with_max_duration(Duration::from_secs(1)),
            )
            .unwrap()
        });
        Self {
            root,
            store,
            probe,
            executor: ModelProviderExecutor::new(routes).unwrap(),
        }
    }
    fn conversation(&self) -> ConversationId {
        self.store
            .v4_create_conversation("Context", 1_700_000_000_000)
            .unwrap()
            .conversation_id
    }
    fn submit(&self, cid: &ConversationId, route: &str, input: &str) -> V4SubmitTurn {
        V4SubmitTurn {
            conversation_id: cid.clone(),
            idempotency_key: IdempotencyKey::new(),
            route_id: route.into(),
            input: input.into(),
        }
    }
    fn execute(&self, submit: V4SubmitTurn) {
        let sub = self
            .store
            .v4_admit_turn(&submit, 1_700_000_000_000)
            .unwrap();
        for (from, to) in [
            (RunState::Queued, RunState::Starting),
            (RunState::Starting, RunState::Running),
        ] {
            self.store
                .transition_run(
                    &sub.run_id,
                    from,
                    to,
                    NewRunEvent {
                        kind: "run_started".into(),
                        payload: serde_json::json!({}),
                    },
                )
                .unwrap();
        }
        self.executor
            .execute_v4(&self.store, &sub.run_id, submit, &CancellationToken::new())
            .unwrap();
        self.store
            .transition_run(
                &sub.run_id,
                RunState::Running,
                RunState::Succeeded,
                NewRunEvent {
                    kind: "run_completed".into(),
                    payload: serde_json::json!({}),
                },
            )
            .unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
fn v4_provider_second_turn_uses_full_durable_context_after_reopen() {
    let mut f = Fixture::new();
    let cid = f.conversation();
    f.execute(f.submit(&cid, "abi.fixture", "first private question"));
    f.store = RuntimeStore::open(&RuntimeStore::path_for_state_dir(&f.root)).unwrap();
    f.execute(f.submit(&cid, "abi.fixture", "second question"));
    let prompts = f.probe.prompts.lock().unwrap();
    assert_eq!(prompts[0], "first private question");
    assert!(prompts[1].contains("first private question"));
    assert!(prompts[1].contains(&format!("{}{}", "a".repeat(16_383), "🙂".repeat(100))));
    assert_eq!(prompts[1].matches("second question").count(), 1);
    assert!(prompts[1].len() <= 32 * 1024);
}

#[test]
fn v4_provider_context_never_crosses_conversation_or_route() {
    let f = Fixture::new();
    let a = f.conversation();
    let b = f.conversation();
    f.execute(f.submit(&a, "abi.fixture", "secret in conversation A"));
    f.execute(f.submit(&b, "abi.fixture", "conversation B"));
    f.execute(f.submit(&a, "fm.system", "new route"));
    let prompts = f.probe.prompts.lock().unwrap();
    assert_eq!(prompts[1], "conversation B");
    assert_eq!(prompts[2], "new route");
}

#[test]
fn v4_provider_rejects_substituted_turn_before_provider_execution() {
    let f = Fixture::new();
    let a = f.conversation();
    let request = f.submit(&a, "abi.fixture", "admitted input");
    let sub = f.store.v4_admit_turn(&request, 1_700_000_000_000).unwrap();
    let mut changed = request;
    changed.input = "substituted input".into();
    assert!(
        f.executor
            .execute_v4(&f.store, &sub.run_id, changed, &CancellationToken::new())
            .is_err()
    );
    assert!(f.probe.prompts.lock().unwrap().is_empty());
}
