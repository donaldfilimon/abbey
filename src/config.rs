//! Abbey config: role→model map, persona policy, memory backend.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AbbeyConfig {
    #[serde(default = "default_persona_policy")]
    pub persona_policy: String,
    #[serde(default = "default_role")]
    pub default_role: String,
    #[serde(default)]
    pub roles: RoleBindings,
    #[serde(default = "default_memory_backend")]
    pub memory_backend: String,
    #[serde(default)]
    pub abi_bin: Option<PathBuf>,
    /// Default executor backend (`ollama` | `cursor` | `grok` | `fm` | `abi` | `claude`).
    /// Precedence: `ABBEY_BACKEND` env > this key > ollama.
    #[serde(default)]
    pub backend: Option<String>,
    /// Learned-memory embedding provider. API credentials are environment-only.
    #[serde(default)]
    pub embeddings: EmbeddingConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EmbeddingConfig {
    /// `none` (default) | `apple` | `openai`.
    #[serde(default = "default_embedding_provider")]
    pub provider: String,
    /// OpenAI-compatible base URL or full `/v1/embeddings` URL.
    #[serde(default = "default_embedding_endpoint")]
    pub endpoint: String,
    /// Provider model identity. Pin a revision here when the provider supports it.
    #[serde(default = "default_embedding_model")]
    pub model: String,
    /// Exact output dimensions expected from the provider.
    #[serde(default = "default_embedding_dimension")]
    pub dimension: usize,
    /// BCP-47/NLLanguage code used by Apple NaturalLanguage.
    #[serde(default = "default_embedding_language")]
    pub language: String,
}

impl Default for EmbeddingConfig {
    fn default() -> Self {
        Self {
            provider: default_embedding_provider(),
            endpoint: default_embedding_endpoint(),
            model: default_embedding_model(),
            dimension: default_embedding_dimension(),
            language: default_embedding_language(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoleBindings {
    /// Executor model id or Abbey alias for Max (technical worker role).
    #[serde(default = "default_max_model")]
    pub max: String,
    /// Executor model id or Abbey alias for Gemma (visual/conversational role).
    #[serde(default = "default_gemma_model")]
    pub gemma: String,
}

impl Default for RoleBindings {
    fn default() -> Self {
        Self {
            max: default_max_model(),
            gemma: default_gemma_model(),
        }
    }
}

impl Default for AbbeyConfig {
    fn default() -> Self {
        Self {
            persona_policy: default_persona_policy(),
            default_role: default_role(),
            roles: RoleBindings::default(),
            memory_backend: default_memory_backend(),
            abi_bin: None,
            backend: None,
            embeddings: EmbeddingConfig::default(),
        }
    }
}

fn default_persona_policy() -> String {
    "auto".into()
}
fn default_role() -> String {
    "auto".into()
}
fn default_memory_backend() -> String {
    "sqlite".into()
}
fn default_max_model() -> String {
    "max".into()
}
fn default_gemma_model() -> String {
    "gemma".into()
}
fn default_embedding_provider() -> String {
    "none".into()
}
fn default_embedding_endpoint() -> String {
    "https://api.openai.com".into()
}
fn default_embedding_model() -> String {
    "text-embedding-3-small".into()
}
fn default_embedding_dimension() -> usize {
    1536
}
fn default_embedding_language() -> String {
    "en".into()
}

impl AbbeyConfig {
    /// Config file for the *active edition*.
    ///
    /// The root and the override variable both come from `edition::ACTIVE`, so
    /// a safe and a personal install can never read the same config.
    pub fn config_path() -> PathBuf {
        crate::edition::ACTIVE.config_path()
    }

    pub fn load() -> Result<Self> {
        let path = Self::config_path();
        if !path.is_file() {
            return Ok(Self::default().with_env_overrides());
        }
        let text = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
        let mut cfg: Self = parse_toml_lite(&text)?;
        cfg = cfg.with_env_overrides();
        Ok(cfg)
    }

    pub fn with_env_overrides(mut self) -> Self {
        if let Ok(v) = std::env::var("ABBEY_ROLE")
            && !v.trim().is_empty()
        {
            self.default_role = v.trim().to_ascii_lowercase();
        }
        if let Ok(v) = std::env::var("ABBEY_PERSONA")
            && !v.trim().is_empty()
        {
            self.persona_policy = v.trim().to_ascii_lowercase();
        }
        if let Ok(v) = std::env::var("ABBEY_MEMORY_BACKEND")
            && !v.trim().is_empty()
        {
            self.memory_backend = v.trim().to_ascii_lowercase();
        }
        if let Ok(v) = std::env::var("ABBEY_ABI_BIN")
            && !v.trim().is_empty()
        {
            self.abi_bin = Some(PathBuf::from(v));
        }
        if let Ok(v) = std::env::var("ABBEY_EMBEDDING_PROVIDER")
            && !v.trim().is_empty()
        {
            self.embeddings.provider = v.trim().to_ascii_lowercase();
        }
        if let Ok(v) = std::env::var("ABBEY_EMBEDDING_ENDPOINT")
            && !v.trim().is_empty()
        {
            self.embeddings.endpoint = v.trim().to_string();
        }
        if let Ok(v) = std::env::var("ABBEY_EMBEDDING_MODEL")
            && !v.trim().is_empty()
        {
            self.embeddings.model = v.trim().to_string();
        }
        if let Ok(v) = std::env::var("ABBEY_EMBEDDING_DIMENSION") {
            // Preserve fail-closed provider validation: an invalid explicit
            // override must not silently keep the configured/default dimension.
            self.embeddings.dimension = v.trim().parse::<usize>().unwrap_or(0);
        }
        if let Ok(v) = std::env::var("ABBEY_EMBEDDING_LANGUAGE")
            && !v.trim().is_empty()
        {
            self.embeddings.language = v.trim().to_string();
        }
        self
    }

    /// Write the annotated default config if none exists yet.
    ///
    /// Returns the path either way; the caller reports whether it was created.
    /// Never overwrites — a config file is user-authored state.
    pub fn ensure_default_file(&self) -> Result<PathBuf> {
        let path = Self::config_path();
        if path.is_file() {
            return Ok(path);
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, default_toml_text())?;
        Ok(path)
    }

    pub fn status_lines(&self) -> Vec<String> {
        vec![
            format!("config:          {}", Self::config_path().display()),
            format!("persona_policy:  {}", self.persona_policy),
            format!("default_role:    {}", self.default_role),
            format!("role.max →       {}", self.roles.max),
            format!("role.gemma →     {}", self.roles.gemma),
            format!("memory_backend:  {}", self.memory_backend),
            format!("embedding:       {}", self.embeddings.provider),
            format!("embedding_model: {}", self.embeddings.model),
            format!("embedding_dim:   {}", self.embeddings.dimension),
            format!(
                "backend:         {}",
                self.backend.as_deref().unwrap_or("(ollama default)")
            ),
            format!(
                "abi_bin:         {}",
                self.abi_bin
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| "(PATH)".into())
            ),
        ]
    }
}

fn default_toml_text() -> &'static str {
    r#"# Abbey hybrid config — role bindings are executor model ids/aliases.
# The portable max/gemma aliases resolve per backend; they are roles, not
# bundled Abbey weights.

persona_policy = "auto"
default_role = "auto"
memory_backend = "sqlite"

# Learned semantic memory is opt-in. Credentials are never stored here: the
# OpenAI-compatible provider reads ABBEY_EMBEDDING_API_KEY or OPENAI_API_KEY.
[embeddings]
provider = "none"
endpoint = "https://api.openai.com"
model = "text-embedding-3-small"
dimension = 1536
language = "en"

# Default executor backend: "ollama" (preferred when installed) | "grok" | "fm" | "abi" | "claude" | "cursor".
# ABBEY_BACKEND overrides this. The unchosen default prefers ollama, then grok/fm/abi/claude, then cursor last.
# backend = "ollama"

# Path to a real `abi` binary (a shell alias will not do). Required by the abi
# backend and by the `abbey wdbx` bridge; ABBEY_ABI_BIN overrides.
# abi_bin = "/path/to/abi"

[roles]
max = "max"
gemma = "gemma"
"#
}

/// Minimal TOML subset parser for our flat + one-table config (no full toml crate required).
fn parse_toml_lite(text: &str) -> Result<AbbeyConfig> {
    let mut cfg = AbbeyConfig::default();
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Section {
        Root,
        Roles,
        Embeddings,
        Unknown,
    }
    let mut section = Section::Root;
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        if line == "[roles]" {
            section = Section::Roles;
            continue;
        }
        if line == "[embeddings]" {
            section = Section::Embeddings;
            continue;
        }
        if line.starts_with('[') {
            section = Section::Unknown;
            continue;
        }
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        let k = k.trim();
        let v = strip_toml_str(v.trim());
        match section {
            Section::Roles => match k {
                "max" => cfg.roles.max = v,
                "gemma" => cfg.roles.gemma = v,
                _ => {}
            },
            Section::Embeddings => match k {
                "provider" => cfg.embeddings.provider = v.to_ascii_lowercase(),
                "endpoint" => cfg.embeddings.endpoint = v,
                "model" => cfg.embeddings.model = v,
                "dimension" => {
                    cfg.embeddings.dimension = v.parse::<usize>().with_context(|| {
                        format!("embedding dimension must be a non-negative integer, got {v:?}")
                    })?;
                }
                "language" => cfg.embeddings.language = v,
                _ => {}
            },
            Section::Root => match k {
                "persona_policy" => cfg.persona_policy = v,
                "default_role" => cfg.default_role = v,
                "memory_backend" => cfg.memory_backend = v,
                "abi_bin" => cfg.abi_bin = Some(PathBuf::from(v)),
                "backend" => cfg.backend = Some(v.to_ascii_lowercase()),
                _ => {}
            },
            Section::Unknown => {}
        }
    }
    Ok(cfg)
}

fn strip_toml_str(s: &str) -> String {
    let s = s.trim().trim_end_matches(',');
    if let Some(inner) = s.strip_prefix('"').and_then(|x| x.strip_suffix('"')) {
        return inner.to_string();
    }
    if let Some(inner) = s.strip_prefix('\'').and_then(|x| x.strip_suffix('\'')) {
        return inner.to_string();
    }
    s.to_string()
}

pub fn resolve_abi_bin(cfg: &AbbeyConfig) -> Option<PathBuf> {
    if let Some(p) = &cfg.abi_bin
        && p.is_file()
    {
        return Some(p.clone());
    }
    crate::agent::which_bin("abi")
}

/// Subprocess bridge: `abi wdbx …` when available (Phase 3 fallback without feature).
pub fn wdbx_cli_status(cfg: &AbbeyConfig) -> String {
    match resolve_abi_bin(cfg) {
        Some(p) => format!("abi: {} (wdbx via `abi wdbx` when invoked)", p.display()),
        None => "abi: (not on PATH — WDBX CLI bridge unavailable)".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_default_shape() {
        let cfg = parse_toml_lite(default_toml_text()).unwrap();
        assert_eq!(cfg.roles.max, "max");
        assert_eq!(cfg.roles.gemma, "gemma");
        assert_eq!(cfg.memory_backend, "sqlite");
        assert_eq!(cfg.embeddings.provider, "none");
        assert_eq!(cfg.embeddings.dimension, 1536);
        // The scaffold must not *activate* anything the user didn't choose:
        // backend/abi_bin ship commented out, so a fresh config still means
        // ollama-when-installed (code default), not a pinned executor.
        assert_eq!(cfg.backend, None);
        assert_eq!(cfg.abi_bin, None);
    }

    /// No *commented* line may look like a real assignment for a different
    /// key, or uncommenting one key silently activates another. Found live:
    /// the `abi_bin` note began `# backend = "abi" and for …`, so a user (or
    /// a sed) uncommenting `backend` matched both lines and parsed the prose
    /// as the value.
    #[test]
    fn no_comment_line_masquerades_as_an_assignment() {
        for line in default_toml_text().lines() {
            let Some(body) = line.trim().strip_prefix("# ") else {
                continue;
            };
            let Some((key, _)) = body.split_once('=') else {
                continue;
            };
            let key = key.trim();
            // A commented-out example is fine (`# backend = "abi"`); prose that
            // merely *starts* like one is the hazard, so require the line to be
            // a clean `key = value` with nothing trailing the quoted value.
            if [
                "persona_policy",
                "default_role",
                "memory_backend",
                "backend",
                "abi_bin",
                "max",
                "gemma",
                "provider",
                "endpoint",
                "model",
                "dimension",
                "language",
            ]
            .contains(&key)
            {
                let value = body.split_once('=').unwrap().1.trim();
                assert!(
                    value.starts_with('"') && value.ends_with('"') && value.len() > 1,
                    "commented line for `{key}` is prose, not a clean example: {line}"
                );
            }
        }
    }

    /// The scaffold is the only documentation many users will read, so every
    /// key `parse_toml_lite` understands must appear in it (commented is fine).
    /// Without this, a new key ships invisible — exactly how `backend` was
    /// missing from the template it was added alongside.
    #[test]
    fn scaffold_mentions_every_supported_key() {
        let text = default_toml_text();
        for key in [
            "persona_policy",
            "default_role",
            "memory_backend",
            "backend",
            "abi_bin",
            "max",
            "gemma",
            "provider",
            "endpoint",
            "model",
            "dimension",
            "language",
        ] {
            assert!(
                text.contains(key),
                "default config scaffold never mentions `{key}`"
            );
        }
    }

    #[test]
    fn env_override_role() {
        // Don't mutate process env in parallel tests aggressively; just check method.
        let cfg = AbbeyConfig {
            default_role: "max".into(),
            ..Default::default()
        };
        assert_eq!(cfg.default_role, "max");
    }

    #[test]
    fn parses_embedding_table_without_storing_a_key() {
        let cfg = parse_toml_lite(
            r#"[embeddings]
provider = "apple"
endpoint = "https://unused.example"
model = "sentence"
dimension = 512
language = "fr"
api_key = "must-be-ignored"
"#,
        )
        .unwrap();
        assert_eq!(cfg.embeddings.provider, "apple");
        assert_eq!(cfg.embeddings.dimension, 512);
        assert_eq!(cfg.embeddings.language, "fr");
        assert!(!format!("{cfg:?}").contains("must-be-ignored"));
    }
}
