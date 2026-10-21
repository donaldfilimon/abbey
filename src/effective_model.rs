//! Read-only "effective model" summary for the current session.
//!
//! One question, one helper, two surfaces: *which model would a turn use
//! right now?* The CLI `doctor` output and the TUI doctor panel both render
//! [`effective_model_line`], so the two cannot disagree.
//!
//! # Rules (reused, never reimplemented)
//!
//! | backend | effective model | canonical helper |
//! | --- | --- | --- |
//! | `fm` | `system` or `pcc` — never a cursor `claude-*` id | [`crate::agent::fm_model`] |
//! | `abi` | the literal selected model (`local`, `live`, a bare `claude-*` catalog id) | [`crate::agent::abi_normalize_model`] |
//! | `ollama` | the normalized local tag (`gemma4:26b-mlx`) | [`crate::agent::ollama_normalize_model`] |
//! | `cursor` / `grok` / `claude` | the alias-table resolution | [`crate::models::resolve_model`] |
//!
//! These are the same normalizers the spawn path uses
//! (`src/agent/argv.rs`) and the same ones `models::select_model` applies, so
//! the summary cannot drift into a second, private model grammar.
//!
//! An unset (empty) model is the `auto` sentinel. Backends with a concrete
//! default name it — `fm` → `system`, `abi` → `local`, `ollama` → the local
//! `gemma4:26b-mlx` tag — because that *is* what a turn would use; backends
//! that let the executor choose report `auto`.
//!
//! # Boundaries
//!
//! * **Live values only.** The backend and the model are passed in by the
//!   caller. This module never reads `AgentBackend::from_env()` — the
//!   once-per-process cache — because the TUI can change `cfg.backend` at
//!   runtime (Ctrl-B), and a cached read would describe a session that no
//!   longer exists.
//! * **A summary, not the argv.** Under `claude` the Claude Code CLI clamps
//!   Abbey's aliases into its own vocabulary when the argv is built, so the
//!   alias-table id shown here is the id Abbey resolved, not the flag the CLI
//!   receives. Under `cursor`/`grok` the two agree, because every selection
//!   path (`session::apply_global_flags`, `models::select_model`, the `/model`
//!   and `/think` slashes) already stores the alias-table resolution.
//! * **Read-only.** Nothing here writes state, persists a preference, selects
//!   a default chat model, downloads or loads a model, or changes routing. It
//!   reports; it does not act.

use crate::agent::AgentBackend;

/// The model a turn would use right now, given the live backend and the live
/// model string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectiveModel {
    /// The live backend a turn would use.
    pub backend: AgentBackend,
    /// The live model string, trimmed. Empty means "unset".
    pub raw: String,
    /// The model id the backend would actually receive.
    pub resolved: String,
}

impl EffectiveModel {
    /// Resolve the effective model for a live backend plus a live model string.
    ///
    /// `raw_model` is the caller's live `cfg.model` — never a re-read of the
    /// environment and never the process-cached backend.
    pub fn resolve(backend: AgentBackend, raw_model: &str) -> Self {
        let raw = raw_model.trim();
        // An unset model is the `auto` sentinel; every backend rule below
        // already knows what to do with it.
        let requested = if raw.is_empty() { "auto" } else { raw };
        let resolved = match backend {
            AgentBackend::Fm => crate::agent::fm_model(requested).to_string(),
            AgentBackend::Abi => crate::agent::abi_normalize_model(requested),
            AgentBackend::Ollama => crate::agent::ollama_normalize_model(requested),
            AgentBackend::Cursor | AgentBackend::Grok | AgentBackend::Claude => {
                crate::models::resolve_model(requested)
            }
        };
        Self {
            backend,
            raw: raw.to_string(),
            resolved,
        }
    }

    /// The live model string as displayed: `(unset)` when nothing is set.
    pub fn raw_display(&self) -> &str {
        if self.raw.is_empty() {
            "(unset)"
        } else {
            &self.raw
        }
    }

    /// The single doctor line, shared by the CLI and the TUI.
    pub fn line(&self) -> String {
        format!(
            "effective model: {}  (live: {}, backend: {})",
            self.resolved,
            self.raw_display(),
            self.backend.label()
        )
    }
}

/// The shared doctor line: the model a turn would use right now.
///
/// Both `abbey doctor` and the TUI doctor panel call this, so the two
/// surfaces report the same thing by construction.
pub fn effective_model_line(backend: AgentBackend, raw_model: &str) -> String {
    EffectiveModel::resolve(backend, raw_model).line()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fm_never_reports_a_cursor_model_id() {
        // `fm` speaks exactly two models; a cursor id must collapse onto them.
        assert_eq!(
            EffectiveModel::resolve(AgentBackend::Fm, "claude-opus-5-thinking-high").resolved,
            "system"
        );
        assert_eq!(
            EffectiveModel::resolve(AgentBackend::Fm, "opus").resolved,
            "system"
        );
        assert_eq!(
            EffectiveModel::resolve(AgentBackend::Fm, "pcc").resolved,
            "pcc"
        );
    }

    #[test]
    fn abi_keeps_the_literal_selection() {
        // No alias expansion that would look like an explicit Anthropic live
        // request: a role/thinking binding stays the local template.
        assert_eq!(
            EffectiveModel::resolve(AgentBackend::Abi, "fable").resolved,
            "local"
        );
        assert_eq!(
            EffectiveModel::resolve(AgentBackend::Abi, "claude-opus-5-thinking-high").resolved,
            "local"
        );
        // A bare catalog id stays literal — it *is* the live transport.
        assert_eq!(
            EffectiveModel::resolve(AgentBackend::Abi, "claude-sonnet-4").resolved,
            "claude-sonnet-4"
        );
        assert_eq!(
            EffectiveModel::resolve(AgentBackend::Abi, "live").resolved,
            "live"
        );
    }

    #[test]
    fn ollama_reports_the_normalized_local_tag() {
        assert_eq!(
            EffectiveModel::resolve(AgentBackend::Ollama, "gemma").resolved,
            crate::models::OLLAMA_DEFAULT_MODEL
        );
        assert_eq!(
            EffectiveModel::resolve(AgentBackend::Ollama, "claude-opus-5-thinking-high").resolved,
            crate::models::OLLAMA_DEFAULT_MODEL
        );
        // A real local tag passes through untouched.
        assert_eq!(
            EffectiveModel::resolve(AgentBackend::Ollama, "gemma4:12b-mlx").resolved,
            "gemma4:12b-mlx"
        );
    }

    #[test]
    fn other_backends_use_the_alias_table() {
        for backend in [
            AgentBackend::Cursor,
            AgentBackend::Grok,
            AgentBackend::Claude,
        ] {
            assert_eq!(
                EffectiveModel::resolve(backend, "opus").resolved,
                "claude-opus-5-thinking-high"
            );
            assert_eq!(EffectiveModel::resolve(backend, "auto").resolved, "auto");
        }
    }

    #[test]
    fn an_unset_model_falls_back_to_auto() {
        // Empty is the unset sentinel. Backends with a concrete default name
        // it; the rest report `auto` rather than inventing a model.
        assert_eq!(
            EffectiveModel::resolve(AgentBackend::Cursor, "").resolved,
            "auto"
        );
        assert_eq!(
            EffectiveModel::resolve(AgentBackend::Fm, "").resolved,
            "system"
        );
        assert_eq!(
            EffectiveModel::resolve(AgentBackend::Abi, "").resolved,
            "local"
        );
        assert_eq!(
            EffectiveModel::resolve(AgentBackend::Ollama, "").resolved,
            crate::models::OLLAMA_DEFAULT_MODEL
        );
    }

    #[test]
    fn resolution_is_idempotent() {
        // The live model string is already normalized on most paths, so
        // resolving twice must not change the answer.
        for backend in [
            AgentBackend::Cursor,
            AgentBackend::Grok,
            AgentBackend::Fm,
            AgentBackend::Abi,
            AgentBackend::Claude,
            AgentBackend::Ollama,
        ] {
            for raw in ["auto", "opus", "fable", "gemma", "pcc", "local"] {
                let once = EffectiveModel::resolve(backend, raw).resolved;
                let twice = EffectiveModel::resolve(backend, &once).resolved;
                assert_eq!(once, twice, "{backend:?} {raw} is not idempotent");
            }
        }
    }

    #[test]
    fn the_line_names_the_resolved_model_the_live_value_and_the_backend() {
        let line = effective_model_line(AgentBackend::Ollama, "gemma");
        assert_eq!(
            line,
            "effective model: gemma4:26b-mlx  (live: gemma, backend: ollama)"
        );
        let unset = effective_model_line(AgentBackend::Cursor, "");
        assert_eq!(
            unset,
            "effective model: auto  (live: (unset), backend: cursor-agent)"
        );
    }
}
