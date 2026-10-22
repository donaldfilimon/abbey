use super::*;

#[test]
fn constructor_keeps_resolved_model_instead_of_persisted_preference() {
    for (backend, selected, persisted, expected) in [
        (AgentBackend::Fm, "pcc", "system", "pcc"),
        (AgentBackend::Fm, "private-cloud-compute", "system", "pcc"),
        (AgentBackend::Fm, "auto", "pcc", "system"),
        (AgentBackend::Ollama, "custom:tag", "gemma", "custom:tag"),
        (AgentBackend::Abi, "claude-opus-5", "local", "claude-opus-5"),
    ] {
        let mut old = scratch_app("constructor-model", backend);
        old.state.save_model_literal(persisted).unwrap();
        let dir = old.state.state_dir.clone();
        let mut cfg = old.cfg.clone();
        cfg.model = selected.into();
        let mut app = App::new(old.state.clone(), cfg).unwrap();
        assert_eq!(app.cfg.model, expected, "{backend:?}");
        // The background refresh writes into the state directory, so it
        // has to be done before that directory is removed.
        assert!(app.join_refresh(), "the refresh worker did not finish");
        assert!(old.join_refresh(), "the refresh worker did not finish");
        drop(app);
        drop(old);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
