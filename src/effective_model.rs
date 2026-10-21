//! Read-only "effective model" summary for the current session.
//!
//! One question, one helper, three surfaces: *which model would a turn use
//! right now?* The CLI `doctor` output, the TUI doctor panel, and the desktop
//! client's Doctor panel all render the same resolution, so they cannot
//! disagree.
//!
//! # Rules (reused, never reimplemented)
//!
//! | backend | effective model | canonical helper |
//! | --- | --- | --- |
//! | `fm` | `system` or `pcc` — never a cursor `claude-*` id | `crate::agent::fm_model` |
//! | `abi` | the literal selected model (`local`, `live`, a bare `claude-*` catalog id) | `crate::agent::abi_normalize_model` |
//! | `ollama` | the normalized local tag (`gemma4:26b-mlx`) | `crate::agent::ollama_normalize_model` |
//! | `cursor` / `grok` / `claude` | the alias-table resolution | `crate::models::resolve_model` |
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
//!   longer exists. The desktop client resolves the same way, from the
//!   runtime's own published route rather than from its own environment.
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

/// Where an [`EffectiveModel`] answer came from.
///
/// Two surfaces answer the same question from two different authorities, and
/// both are legitimate: the CLI and the TUI resolve from the *live local
/// configuration* (`cfg.backend` / `cfg.model`), while the desktop client
/// resolves from the *runtime's own published route* — the route the daemon
/// will actually run a turn on. This discriminator makes that difference
/// explicit so a consumer can never mistake one authority for the other, and
/// so a client can say which one it consulted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EffectiveModelSource {
    /// The live local configuration of the process asking.
    LocalConfig,
    /// A route the connected runtime published.
    RuntimeRoute,
}

impl EffectiveModelSource {
    /// A stable, short label for logs and diagnostics.
    pub fn label(self) -> &'static str {
        match self {
            Self::LocalConfig => "local-config",
            Self::RuntimeRoute => "runtime-route",
        }
    }
}

/// The model a turn would use right now, given the live backend and the live
/// model string.
///
/// The backend is carried as its canonical label (`ollama`, `cursor-agent`,
/// `grok`, `fm`, `abi`, `claude`) rather than as the executor-selection
/// enum, so this contract crosses the crate boundary to the desktop client
/// without exposing the executor-selection internals. The backend parser is
/// the one place a label becomes a backend, so there is still exactly one
/// backend vocabulary.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct EffectiveModel {
    /// The canonical backend label a turn would use.
    pub backend: String,
    /// The live model string, trimmed. Empty means "unset".
    pub raw: String,
    /// The model id the backend would actually receive.
    pub resolved: String,
    /// Which authority this answer was resolved against.
    pub source: EffectiveModelSource,
}

impl EffectiveModel {
    /// Resolve the effective model for a canonical backend label plus a live
    /// model string.
    ///
    /// `backend` is one of the backend parser's labels; an unknown label
    /// resolves to `None` rather than to a guessed backend. `raw_model` is the
    /// caller's live model string — never a re-read of the environment and
    /// never the process-cached backend.
    pub fn resolve(backend: &str, raw_model: &str) -> Option<Self> {
        let parsed = AgentBackend::parse(backend)?;
        Some(Self::resolve_backend(parsed, raw_model))
    }

    /// Resolve against a route the connected runtime published.
    ///
    /// The same rules as [`Self::resolve`], with the authority recorded as
    /// the runtime's own route. A route's backend is already a canonical
    /// label and its model is already the exact id the provider receives, so
    /// the resolution is the shared one and not a second grammar.
    pub fn resolve_for_route(backend: &str, model: &str) -> Option<Self> {
        let mut resolved = Self::resolve(backend, model)?;
        resolved.source = EffectiveModelSource::RuntimeRoute;
        Some(resolved)
    }

    /// Resolve for an already-parsed live backend.
    ///
    /// The typed entry point the CLI and the TUI use; it is the same rules
    /// [`Self::resolve`] applies after parsing a label.
    pub(crate) fn resolve_backend(backend: AgentBackend, raw_model: &str) -> Self {
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
            backend: backend.label().to_string(),
            raw: raw.to_string(),
            resolved,
            source: EffectiveModelSource::LocalConfig,
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

    /// The single doctor line, shared by the CLI, the TUI, and the desktop.
    ///
    /// The line does *not* name the source. The CLI and the TUI always
    /// resolve against the live local configuration, so for them the
    /// authority is not in question; the desktop resolves against a runtime
    /// route and renders the source itself, from [`Self::source`]. Keeping
    /// the line free of the discriminator also keeps the CLI's doctor output
    /// stable for anything that reads it.
    pub fn line(&self) -> String {
        format!(
            "effective model: {}  (live: {}, backend: {})",
            self.resolved,
            self.raw_display(),
            self.backend
        )
    }
}

/// The shared doctor line: the model a turn would use right now.
///
/// Both `abbey doctor` and the TUI doctor panel call this, so the two
/// surfaces report the same thing by construction.
pub(crate) fn effective_model_line(backend: AgentBackend, raw_model: &str) -> String {
    EffectiveModel::resolve_backend(backend, raw_model).line()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fm_never_reports_a_cursor_model_id() {
        // `fm` speaks exactly two models; a cursor id must collapse onto them.
        assert_eq!(
            EffectiveModel::resolve_backend(AgentBackend::Fm, "claude-opus-5-thinking-high")
                .resolved,
            "system"
        );
        assert_eq!(
            EffectiveModel::resolve_backend(AgentBackend::Fm, "opus").resolved,
            "system"
        );
        assert_eq!(
            EffectiveModel::resolve_backend(AgentBackend::Fm, "pcc").resolved,
            "pcc"
        );
    }

    #[test]
    fn abi_keeps_the_literal_selection() {
        // No alias expansion that would look like an explicit Anthropic live
        // request: a role/thinking binding stays the local template.
        assert_eq!(
            EffectiveModel::resolve_backend(AgentBackend::Abi, "fable").resolved,
            "local"
        );
        assert_eq!(
            EffectiveModel::resolve_backend(AgentBackend::Abi, "claude-opus-5-thinking-high")
                .resolved,
            "local"
        );
        // A bare catalog id stays literal — it *is* the live transport.
        assert_eq!(
            EffectiveModel::resolve_backend(AgentBackend::Abi, "claude-sonnet-4").resolved,
            "claude-sonnet-4"
        );
        assert_eq!(
            EffectiveModel::resolve_backend(AgentBackend::Abi, "live").resolved,
            "live"
        );
    }

    #[test]
    fn ollama_reports_the_normalized_local_tag() {
        assert_eq!(
            EffectiveModel::resolve_backend(AgentBackend::Ollama, "gemma").resolved,
            crate::models::OLLAMA_DEFAULT_MODEL
        );
        assert_eq!(
            EffectiveModel::resolve_backend(AgentBackend::Ollama, "claude-opus-5-thinking-high")
                .resolved,
            crate::models::OLLAMA_DEFAULT_MODEL
        );
        // A real local tag passes through untouched.
        assert_eq!(
            EffectiveModel::resolve_backend(AgentBackend::Ollama, "gemma4:12b-mlx").resolved,
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
                EffectiveModel::resolve_backend(backend, "opus").resolved,
                "claude-opus-5-thinking-high"
            );
            assert_eq!(
                EffectiveModel::resolve_backend(backend, "auto").resolved,
                "auto"
            );
        }
    }

    #[test]
    fn an_unset_model_falls_back_to_auto() {
        // Empty is the unset sentinel. Backends with a concrete default name
        // it; the rest report `auto` rather than inventing a model.
        assert_eq!(
            EffectiveModel::resolve_backend(AgentBackend::Cursor, "").resolved,
            "auto"
        );
        assert_eq!(
            EffectiveModel::resolve_backend(AgentBackend::Fm, "").resolved,
            "system"
        );
        assert_eq!(
            EffectiveModel::resolve_backend(AgentBackend::Abi, "").resolved,
            "local"
        );
        assert_eq!(
            EffectiveModel::resolve_backend(AgentBackend::Ollama, "").resolved,
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
                let once = EffectiveModel::resolve_backend(backend, raw).resolved;
                let twice = EffectiveModel::resolve_backend(backend, &once).resolved;
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

    #[test]
    fn the_source_names_the_authority_the_answer_came_from() {
        // The CLI and the TUI resolve from the live local configuration.
        let local = EffectiveModel::resolve("ollama", "gemma").expect("a known backend");
        assert_eq!(local.source, EffectiveModelSource::LocalConfig);
        assert_eq!(local.source.label(), "local-config");

        // The desktop resolves from a route the connected runtime published.
        let route = EffectiveModel::resolve_for_route("abi", "local").expect("a known backend");
        assert_eq!(route.source, EffectiveModelSource::RuntimeRoute);
        assert_eq!(route.source.label(), "runtime-route");

        // The two authorities resolve the *same* model for the same pair:
        // the source differs, the answer does not.
        let typed = EffectiveModel::resolve_backend(AgentBackend::Ollama, "gemma");
        assert_eq!(typed.source, EffectiveModelSource::LocalConfig);
        assert_eq!(typed.resolved, local.resolved);
    }

    #[test]
    fn a_route_resolution_matches_the_local_resolution_of_the_same_pair() {
        // The route's backend and model are already canonical, so resolving
        // them is the shared grammar and not a second one.
        for (backend, model) in [
            ("abi", "local"),
            ("fm", "claude-opus-5-thinking-high"),
            ("ollama", "gemma"),
            ("cursor-agent", "opus"),
        ] {
            let local = EffectiveModel::resolve(backend, model).expect("a known backend");
            let route = EffectiveModel::resolve_for_route(backend, model).expect("a known backend");
            assert_eq!(route.resolved, local.resolved, "backend {backend}");
            assert_eq!(route.raw, local.raw, "backend {backend}");
            assert_eq!(route.backend, local.backend, "backend {backend}");
        }
    }

    #[test]
    fn a_route_resolution_refuses_an_unknown_backend() {
        assert!(EffectiveModel::resolve_for_route("nonsense", "gemma").is_none());
    }

    #[test]
    fn a_label_resolves_exactly_like_the_typed_backend() {
        // The public, string-keyed entry point the desktop client uses must
        // agree with the typed path the CLI and the TUI use.
        for backend in [
            AgentBackend::Cursor,
            AgentBackend::Grok,
            AgentBackend::Fm,
            AgentBackend::Abi,
            AgentBackend::Claude,
            AgentBackend::Ollama,
        ] {
            for raw in ["auto", "opus", "fable", "gemma", "pcc", "local", ""] {
                let typed = EffectiveModel::resolve_backend(backend, raw);
                let labeled = EffectiveModel::resolve(backend.label(), raw)
                    .expect("a canonical label always resolves");
                assert_eq!(labeled, typed, "{backend:?} {raw} disagrees");
                assert_eq!(labeled.backend, backend.label());
            }
        }
    }

    #[test]
    fn an_unknown_backend_label_resolves_to_nothing() {
        // A label nobody can spell must not be guessed into a backend.
        assert!(EffectiveModel::resolve("", "gemma").is_none());
        assert!(EffectiveModel::resolve("foundation_models", "system").is_none());
        assert!(EffectiveModel::resolve("nonsense", "gemma").is_none());
    }
}
