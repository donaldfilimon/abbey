//! Abbey configuration snapshot: a read-only view of config plus the live
//! session, with honest provenance for every value.
//!
//! Split out of [`crate::config`] so that file parsing and the snapshot
//! surface live in separate modules. The public paths are unchanged:
//! `config::ConfigSnapshot`, `config::ConfigSource`, `config::ConfigField`,
//! `config::LiveSession`, `config::AbbeyConfig::live_session`, and
//! `config::AbbeyConfig::snapshot` are all re-exported from `config.rs`.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;

use crate::agent::AgentConfig;
use crate::config::AbbeyConfig;
use crate::state::AbbeyState;

/// The built-in default configuration.
///
/// `AbbeyConfig` has a private inherent `default()` beside its `Default`
/// impl; both build the same value, but the inherent one is not visible from
/// this module, so the trait impl is named explicitly here.
fn default_config() -> AbbeyConfig {
    <AbbeyConfig as Default>::default()
}

/// Where a configuration value originated, for honest diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigSource {
    /// The value was read from config.toml.
    File,
    /// The value is the built-in default (nothing in file or env set it).
    Default,
    /// The value was overridden by an environment variable.
    Env,
}

impl ConfigSource {
    pub fn label(self) -> &'static str {
        match self {
            ConfigSource::File => "file",
            ConfigSource::Default => "default",
            ConfigSource::Env => "env",
        }
    }
}

/// A single configuration value with its provenance.
#[derive(Debug, Clone)]
pub struct ConfigField {
    pub value: String,
    pub source: ConfigSource,
}
/// The live, in-process session selection (no executor probe).
#[derive(Debug, Clone)]
pub struct LiveSession {
    pub backend: String,
    pub model: String,
    pub state_dir: PathBuf,
    pub cwd: PathBuf,
    pub per_cwd: bool,
}
/// A read-only snapshot of configuration plus live session state, with
/// provenance for every value. No secrets are ever included.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ConfigSnapshot {
    pub config_path: PathBuf,
    pub parse_errors: Vec<String>,
    pub live: LiveSession,
    pub backend: ConfigField,
    pub persona_policy: ConfigField,
    pub default_role: ConfigField,
    pub roles: HashMap<String, ConfigField>,
    pub memory_backend: ConfigField,
    pub abi_bin: Option<ConfigField>,
    pub embeddings: HashMap<String, ConfigField>,
    pub knowledge: HashMap<String, ConfigField>,
}

impl AbbeyConfig {
    /// Build a read-only live-session description from the live agent config
    /// and state. Threads `cfg.backend` (the live selection) rather than
    /// `AgentBackend::from_env()` (the cached once-per-process choice).
    pub fn live_session(cfg: &AgentConfig, state: &AbbeyState) -> LiveSession {
        LiveSession {
            backend: cfg.backend.label().to_string(),
            model: cfg.model.clone(),
            state_dir: state.state_dir.clone(),
            cwd: state.cwd.clone(),
            per_cwd: state.per_cwd,
        }
    }

    /// Produce a read-only configuration snapshot with provenance for every
    /// value. Re-reads config.toml to determine file-vs-default provenance and
    /// checks the environment for env overrides. Secrets
    /// (`ABBEY_EMBEDDING_API_KEY`, `OPENAI_API_KEY`) never appear.
    pub fn snapshot(&self, live: &LiveSession) -> ConfigSnapshot {
        let config_path = Self::config_path();
        let (_file_cfg, file_keys, parse_errors) = if config_path.is_file() {
            match fs::read_to_string(&config_path) {
                Ok(text) => match crate::config::parse_toml_lite_with_keys(&text) {
                    Ok((cfg, keys)) => (cfg, keys, Vec::new()),
                    Err(e) => (default_config(), HashSet::new(), vec![e.to_string()]),
                },
                Err(e) => (default_config(), HashSet::new(), vec![e.to_string()]),
            }
        } else {
            (default_config(), HashSet::new(), Vec::new())
        };

        let file_present = |key: &str| file_keys.contains(key);
        let env_present = |var: &str| -> bool {
            std::env::var(var)
                .map(|v| !v.trim().is_empty())
                .unwrap_or(false)
        };
        let source = |env: bool, file: bool| -> ConfigSource {
            if env {
                ConfigSource::Env
            } else if file {
                ConfigSource::File
            } else {
                ConfigSource::Default
            }
        };

        let backend_value = self
            .backend
            .as_deref()
            .unwrap_or("(ollama default)")
            .to_string();
        let abi_value: Option<String> = self.abi_bin.as_ref().map(|p| p.display().to_string());

        ConfigSnapshot {
            config_path,
            parse_errors,
            live: live.clone(),
            backend: ConfigField {
                value: backend_value,
                source: source(false, file_present("backend")),
            },
            persona_policy: ConfigField {
                value: self.persona_policy.clone(),
                source: source(env_present("ABBEY_PERSONA"), file_present("persona_policy")),
            },
            default_role: ConfigField {
                value: self.default_role.clone(),
                source: source(env_present("ABBEY_ROLE"), file_present("default_role")),
            },
            roles: HashMap::from([
                (
                    "max".to_string(),
                    ConfigField {
                        value: self.roles.max.clone(),
                        source: source(false, file_present("roles.max")),
                    },
                ),
                (
                    "gemma".to_string(),
                    ConfigField {
                        value: self.roles.gemma.clone(),
                        source: source(false, file_present("roles.gemma")),
                    },
                ),
            ]),
            memory_backend: ConfigField {
                value: self.memory_backend.clone(),
                source: source(
                    env_present("ABBEY_MEMORY_BACKEND"),
                    file_present("memory_backend"),
                ),
            },
            abi_bin: self.abi_bin.as_ref().map(|_| ConfigField {
                value: abi_value.unwrap_or_default(),
                source: source(env_present("ABBEY_ABI_BIN"), file_present("abi_bin")),
            }),
            knowledge: HashMap::from([
                (
                    "source".to_string(),
                    ConfigField {
                        value: self.knowledge.source.clone(),
                        source: source(false, file_present("knowledge.source")),
                    },
                ),
                (
                    "folder".to_string(),
                    ConfigField {
                        // Do not publish private source paths in diagnostics.
                        value: if self.knowledge.folder.is_some() {
                            "(configured)"
                        } else {
                            "(not configured)"
                        }
                        .to_string(),
                        source: source(false, file_present("knowledge.folder")),
                    },
                ),
            ]),
            embeddings: HashMap::from([
                (
                    "provider".to_string(),
                    ConfigField {
                        value: self.embeddings.provider.clone(),
                        source: source(
                            env_present("ABBEY_EMBEDDING_PROVIDER"),
                            file_present("embeddings.provider"),
                        ),
                    },
                ),
                (
                    "endpoint".to_string(),
                    ConfigField {
                        value: self.embeddings.endpoint.clone(),
                        source: source(
                            env_present("ABBEY_EMBEDDING_ENDPOINT"),
                            file_present("embeddings.endpoint"),
                        ),
                    },
                ),
                (
                    "model".to_string(),
                    ConfigField {
                        value: self.embeddings.model.clone(),
                        source: source(
                            env_present("ABBEY_EMBEDDING_MODEL"),
                            file_present("embeddings.model"),
                        ),
                    },
                ),
                (
                    "dimension".to_string(),
                    ConfigField {
                        value: self.embeddings.dimension.to_string(),
                        source: source(
                            env_present("ABBEY_EMBEDDING_DIMENSION"),
                            file_present("embeddings.dimension"),
                        ),
                    },
                ),
                (
                    "language".to_string(),
                    ConfigField {
                        value: self.embeddings.language.clone(),
                        source: source(
                            env_present("ABBEY_EMBEDDING_LANGUAGE"),
                            file_present("embeddings.language"),
                        ),
                    },
                ),
            ]),
        }
    }
}
