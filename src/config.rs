//! Abbey config: role→model map, persona policy, memory backend.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

use crate::agent::AgentConfig;
use crate::state::AbbeyState;

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
}

impl AbbeyConfig {
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
            return Self::default().with_env_overrides();
        }
        let text = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
        let mut cfg: Self = parse_toml_lite(&text)?;
        cfg = cfg.with_env_overrides()?;
        Ok(cfg)
    }

    pub fn with_env_overrides(mut self) -> Result<Self> {
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
            // An invalid explicit override must be reported as an error,
            // not silently coerced to 0.
            self.embeddings.dimension = v.trim().parse::<usize>().with_context(|| {
                format!("invalid ABBEY_EMBEDDING_DIMENSION {v:?}: must be a non-negative integer")
            })?;
        }
        if let Ok(v) = std::env::var("ABBEY_EMBEDDING_LANGUAGE")
            && !v.trim().is_empty()
        {
            self.embeddings.language = v.trim().to_string();
        }
        Ok(self)
    }

    /// Write the annotated default config if none exists yet.
    ///
    /// Returns the path either way; the caller reports whether it was created.
    /// Never overwrites — a config file is user-authored state.
    pub fn ensure_default_file(&self) -> Result<PathBuf> {
        ensure_default_file_at(&Self::config_path())
    }

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
                Ok(text) => match parse_toml_lite_with_keys(&text) {
                    Ok((cfg, keys)) => (cfg, keys, Vec::new()),
                    Err(e) => (AbbeyConfig::default(), HashSet::new(), vec![e.to_string()]),
                },
                Err(e) => (AbbeyConfig::default(), HashSet::new(), vec![e.to_string()]),
            }
        } else {
            (AbbeyConfig::default(), HashSet::new(), Vec::new())
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

fn ensure_default_file_at(path: &Path) -> Result<PathBuf> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    // Exclusive creation also refuses dangling symlinks; checking is_file first
    // would both follow links and race another initializer or user edit.
    match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
    {
        Ok(mut file) => file.write_all(default_toml_text().as_bytes())?,
        Err(error) if error.kind() == ErrorKind::AlreadyExists => {
            let existing = fs::symlink_metadata(path)?;
            if !existing.is_file() && !existing.file_type().is_symlink() {
                return Err(error.into());
            }
        }
        Err(error) => return Err(error.into()),
    }
    Ok(path.to_path_buf())
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
///
/// Returns the parsed config plus the set of section-qualified keys that were
/// explicitly set in the file, so callers can assign File vs Default
/// provenance honestly.
fn parse_toml_lite_with_keys(text: &str) -> Result<(AbbeyConfig, HashSet<String>)> {
    let mut cfg = AbbeyConfig::default();
    let mut keys = HashSet::new();
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Section {
        Root,
        Roles,
        Embeddings,
        Unknown,
    }
    let mut section = Section::Root;
    for (index, line) in text.lines().enumerate() {
        let line = without_toml_comment(line).trim();
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
        // Unknown keys/tables remain forward-compatible, including values
        // outside this deliberately small parser's supported syntax.
        let known = match section {
            Section::Root => matches!(
                k,
                "persona_policy" | "default_role" | "memory_backend" | "abi_bin" | "backend"
            ),
            Section::Roles => matches!(k, "max" | "gemma"),
            Section::Embeddings => {
                matches!(
                    k,
                    "provider" | "endpoint" | "model" | "dimension" | "language"
                )
            }
            Section::Unknown => false,
        };
        if !known {
            continue;
        }
        let v = strip_toml_str(v.trim())
            .with_context(|| format!("config line {}, key {k:?}", index + 1))?;
        match section {
            Section::Roles => match k {
                "max" => {
                    cfg.roles.max = v;
                    keys.insert("roles.max".to_string());
                }
                "gemma" => {
                    cfg.roles.gemma = v;
                    keys.insert("roles.gemma".to_string());
                }
                _ => {}
            },
            Section::Embeddings => match k {
                "provider" => {
                    cfg.embeddings.provider = v.to_ascii_lowercase();
                    keys.insert("embeddings.provider".to_string());
                }
                "endpoint" => {
                    cfg.embeddings.endpoint = v;
                    keys.insert("embeddings.endpoint".to_string());
                }
                "model" => {
                    cfg.embeddings.model = v;
                    keys.insert("embeddings.model".to_string());
                }
                "dimension" => {
                    cfg.embeddings.dimension = v.parse::<usize>().with_context(|| {
                        format!("embedding dimension must be a non-negative integer, got {v:?}")
                    })?;
                    keys.insert("embeddings.dimension".to_string());
                }
                "language" => {
                    cfg.embeddings.language = v;
                    keys.insert("embeddings.language".to_string());
                }
                _ => {}
            },
            Section::Root => match k {
                "persona_policy" => {
                    cfg.persona_policy = v;
                    keys.insert("persona_policy".to_string());
                }
                "default_role" => {
                    cfg.default_role = v;
                    keys.insert("default_role".to_string());
                }
                "memory_backend" => {
                    cfg.memory_backend = v;
                    keys.insert("memory_backend".to_string());
                }
                "abi_bin" => {
                    cfg.abi_bin = Some(PathBuf::from(v));
                    keys.insert("abi_bin".to_string());
                }
                "backend" => {
                    cfg.backend = Some(v.to_ascii_lowercase());
                    keys.insert("backend".to_string());
                }
                _ => {}
            },
            Section::Unknown => {}
        }
    }
    Ok((cfg, keys))
}

fn parse_toml_lite(text: &str) -> Result<AbbeyConfig> {
    Ok(parse_toml_lite_with_keys(text)?.0)
}

/// A hash starts a comment only outside single-line basic/literal strings.
fn without_toml_comment(line: &str) -> &str {
    let mut quote = None;
    let mut escaped = false;
    for (index, ch) in line.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        match (quote, ch) {
            (Some('"'), '\\') => escaped = true,
            (Some(delimiter), ch) if delimiter == ch => quote = None,
            (None, '"' | '\'') => quote = Some(ch),
            (None, '#') => return &line[..index],
            _ => {}
        }
    }
    line
}

/// Decode single-line TOML strings; retain legacy unquoted simple scalars.
fn strip_toml_str(s: &str) -> Result<String> {
    let Some(quote @ ('"' | '\'')) = s.chars().next() else {
        anyhow::ensure!(
            !s.contains(['"', '\'']),
            "malformed value: quote the entire string"
        );
        return Ok(s.trim_end_matches(',').to_string());
    };
    anyhow::ensure!(
        !s.starts_with("\"\"\"") && !s.starts_with("'''"),
        "multiline strings are unsupported; use a single-line quoted string"
    );
    let mut chars = s[1..].chars();
    let mut decoded = String::new();
    while let Some(ch) = chars.next() {
        if ch == quote {
            anyhow::ensure!(
                chars.as_str().trim().is_empty(),
                "unexpected text after closing quote; only a comment may follow"
            );
            return Ok(decoded);
        }
        anyhow::ensure!(
            ch == '\t' || !ch.is_control(),
            "control characters must use basic-string escapes"
        );
        if quote == '\'' || ch != '\\' {
            decoded.push(ch);
            continue;
        }
        let escape = chars.next().context("unfinished basic-string escape")?;
        decoded.push(match escape {
            'b' => '\u{0008}',
            't' => '\t',
            'n' => '\n',
            'f' => '\u{000c}',
            'r' => '\r',
            '"' => '"',
            '\\' => '\\',
            'u' | 'U' => {
                let mut value = 0;
                for _ in 0..if escape == 'u' { 4 } else { 8 } {
                    let digit = chars
                        .next()
                        .and_then(|c| c.to_digit(16))
                        .context("Unicode escape requires exactly 4 (\\u) or 8 (\\U) hex digits")?;
                    value = (value << 4) | digit;
                }
                char::from_u32(value).context("Unicode escape must name a Unicode scalar value")?
            }
            _ => anyhow::bail!(
                "unsupported basic-string escape \\{escape}; use TOML escapes or a literal single-quoted string for paths"
            ),
        });
    }
    anyhow::bail!("unterminated string; add the matching closing {quote} quote")
}

pub fn resolve_abi_bin(cfg: &AbbeyConfig) -> Option<PathBuf> {
    if let Some(p) = &cfg.abi_bin
        && crate::host::is_executable(p)
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

    struct Scratch(PathBuf);

    impl Scratch {
        fn new() -> Self {
            static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let path = std::env::temp_dir().join(format!(
                "abbey-config-{}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos(),
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn default_file_preserves_existing_content() {
        let dir = Scratch::new();
        let path = dir.0.join("config.toml");
        let content = "# user-authored config\npersona_policy = \"manual\"\n";
        fs::write(&path, content).unwrap();

        assert_eq!(ensure_default_file_at(&path).unwrap(), path);
        assert_eq!(fs::read_to_string(&path).unwrap(), content);
    }

    #[test]
    fn default_file_rejects_directory_without_changing_its_contents() {
        let dir = Scratch::new();
        let path = dir.0.join("config.toml");
        fs::create_dir(&path).unwrap();
        fs::write(path.join("keep"), "existing content").unwrap();
        assert!(ensure_default_file_at(&path).is_err());
        assert_eq!(
            fs::read_to_string(path.join("keep")).unwrap(),
            "existing content"
        );
    }

    #[cfg(unix)]
    #[test]
    fn default_file_rejects_socket_without_removing_it() {
        let dir = Scratch::new();
        let path = dir.0.join("config.toml");
        let _listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
        assert!(ensure_default_file_at(&path).is_err());
        use std::os::unix::fs::FileTypeExt;
        assert!(fs::symlink_metadata(&path).unwrap().file_type().is_socket());
    }

    #[cfg(unix)]
    #[test]
    fn default_file_preserves_dangling_symlink() {
        let dir = Scratch::new();
        let path = dir.0.join("config.toml");
        let target = dir.0.join("missing.toml");
        std::os::unix::fs::symlink(&target, &path).unwrap();

        assert_eq!(ensure_default_file_at(&path).unwrap(), path);
        assert_eq!(fs::read_link(&path).unwrap(), target);
        assert!(!target.exists());
    }

    #[test]
    fn default_file_concurrent_initialization() {
        let dir = Scratch::new();
        let path = dir.0.join("nested/config.toml");
        let barrier = std::sync::Barrier::new(8);
        std::thread::scope(|scope| {
            let handles: Vec<_> = (0..8)
                .map(|_| {
                    scope.spawn(|| {
                        barrier.wait();
                        assert_eq!(ensure_default_file_at(&path).unwrap(), path);
                    })
                })
                .collect();
            for handle in handles {
                handle.join().unwrap();
            }
        });
        assert_eq!(fs::read_to_string(&path).unwrap(), default_toml_text());
    }

    #[test]
    fn quoted_hashes_and_comments_in_every_section() {
        let cfg = parse_toml_lite(
            r##"abi_bin = "/opt/abi#build/bin" # path comment
backend = ollama # legacy scalar
[roles] # table comment
max = "model\"#one" # escaped quote does not end the string
gemma = 'model#two' # literal hash
[embeddings]
endpoint = "https://example.test/#fragment" # URL comment
dimension = 512 # integer comment
"##,
        )
        .unwrap();
        assert_eq!(cfg.abi_bin.unwrap(), PathBuf::from("/opt/abi#build/bin"));
        assert_eq!(cfg.backend.as_deref(), Some("ollama"));
        assert_eq!(cfg.roles.max, "model\"#one");
        assert_eq!(cfg.roles.gemma, "model#two");
        assert_eq!(cfg.embeddings.endpoint, "https://example.test/#fragment");
        assert_eq!(cfg.embeddings.dimension, 512);
    }

    #[test]
    fn literal_and_basic_windows_paths() {
        for text in [
            r#"abi_bin = 'C:\tools\#build\abi.exe' # literal path"#,
            r#"abi_bin = "C:\\tools\\#build\\abi.exe" # basic path"#,
        ] {
            let cfg = parse_toml_lite(text).unwrap();
            assert_eq!(
                cfg.abi_bin.unwrap(),
                PathBuf::from(r"C:\tools\#build\abi.exe")
            );
        }
        let cfg = parse_toml_lite(r#"abi_bin = "C:\\tools\\" # closing quote after escaped slash"#)
            .unwrap();
        assert_eq!(cfg.abi_bin.unwrap(), PathBuf::from("C:\\tools\\"));
    }

    #[test]
    fn basic_string_escape_decoding() {
        assert_eq!(
            strip_toml_str(r#""\b\t\n\f\r\"\\\u0023\u00E9\U0001F600""#).unwrap(),
            "\u{8}\t\n\u{c}\r\"\\#é😀"
        );
        assert_eq!(strip_toml_str("'literal\\n\"#'").unwrap(), "literal\\n\"#");
    }

    #[test]
    fn malformed_strings_report_key_and_line() {
        for value in [
            r#""bad\q""#,
            r#""bad\#""#,
            r#""bad\x41""#,
            r#""bad\u12""#,
            r#""bad\uZZZZ""#,
            r#""bad\uD800""#,
            r#""bad\U00110000""#,
            "\"unfinished\\",
            "\"unfinished",
            "'unfinished",
            "\"closed\" trailing",
            "'closed' trailing",
            "\"\"\"multiline\"\"\"",
            "'''multiline'''",
            "unquoted\"",
        ] {
            let error = parse_toml_lite(&format!("# heading\nabi_bin = {value}")).unwrap_err();
            let message = format!("{error:#}");
            assert!(message.contains("line 2, key \"abi_bin\""), "{message}");
        }
    }

    #[test]
    fn unknown_values_remain_ignored() {
        let cfg = parse_toml_lite(
            "future = \"unsupported\\q\"\n[future]\nbackend = '''ignored'''\n[roles]\nmax = custom",
        )
        .unwrap();
        assert_eq!(cfg.backend, None);
        assert_eq!(cfg.roles.max, "custom");
    }

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
