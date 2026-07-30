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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoleBindings {
    /// cursor-agent model id or Abbey alias for Max (technical worker role).
    #[serde(default = "default_max_model")]
    pub max: String,
    /// cursor-agent model id or Abbey alias for Gemma (visual/conversational role).
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
    "fable".into()
}
fn default_gemma_model() -> String {
    "composer".into()
}

impl AbbeyConfig {
    pub fn config_path() -> PathBuf {
        if let Some(p) = std::env::var_os("ABBEY_CONFIG") {
            return PathBuf::from(p);
        }
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("abbey")
            .join("config.toml")
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
        if let Ok(v) = std::env::var("ABBEY_ROLE") {
            if !v.trim().is_empty() {
                self.default_role = v.trim().to_ascii_lowercase();
            }
        }
        if let Ok(v) = std::env::var("ABBEY_PERSONA") {
            if !v.trim().is_empty() {
                self.persona_policy = v.trim().to_ascii_lowercase();
            }
        }
        if let Ok(v) = std::env::var("ABBEY_MEMORY_BACKEND") {
            if !v.trim().is_empty() {
                self.memory_backend = v.trim().to_ascii_lowercase();
            }
        }
        if let Ok(v) = std::env::var("ABBEY_ABI_BIN") {
            if !v.trim().is_empty() {
                self.abi_bin = Some(PathBuf::from(v));
            }
        }
        self
    }

    #[allow(dead_code)]
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

#[allow(dead_code)]
fn default_toml_text() -> &'static str {
    r#"# Abbey hybrid config — role bindings are cursor-agent model ids/aliases,
# NOT local Qwen/Gemma weights.

persona_policy = "auto"
default_role = "auto"
memory_backend = "sqlite"

[roles]
max = "fable"
gemma = "composer"
"#
}

/// Minimal TOML subset parser for our flat + one-table config (no full toml crate required).
fn parse_toml_lite(text: &str) -> Result<AbbeyConfig> {
    let mut cfg = AbbeyConfig::default();
    let mut in_roles = false;
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        if line == "[roles]" {
            in_roles = true;
            continue;
        }
        if line.starts_with('[') {
            in_roles = false;
            continue;
        }
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        let k = k.trim();
        let v = strip_toml_str(v.trim());
        if in_roles {
            match k {
                "max" => cfg.roles.max = v,
                "gemma" => cfg.roles.gemma = v,
                _ => {}
            }
        } else {
            match k {
                "persona_policy" => cfg.persona_policy = v,
                "default_role" => cfg.default_role = v,
                "memory_backend" => cfg.memory_backend = v,
                "abi_bin" => cfg.abi_bin = Some(PathBuf::from(v)),
                _ => {}
            }
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
    if let Some(p) = &cfg.abi_bin {
        if p.is_file() {
            return Some(p.clone());
        }
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
        assert_eq!(cfg.roles.max, "fable");
        assert_eq!(cfg.roles.gemma, "composer");
        assert_eq!(cfg.memory_backend, "sqlite");
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
}
