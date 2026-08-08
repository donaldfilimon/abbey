use anyhow::{Context, Result, bail};
use serde_json::Value;
use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const PROVIDER_TIMEOUT: Duration = Duration::from_secs(8);
const MAX_CAPTURE_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PluginProvider {
    Cursor,
    Codex,
    Claude,
    Abi,
}

impl PluginProvider {
    pub const ALL: [Self; 4] = [Self::Cursor, Self::Codex, Self::Claude, Self::Abi];

    pub fn label(self) -> &'static str {
        match self {
            Self::Cursor => "cursor",
            Self::Codex => "codex",
            Self::Claude => "claude",
            Self::Abi => "abi",
        }
    }

    fn binary(self) -> &'static str {
        match self {
            Self::Cursor => "cursor-agent",
            Self::Codex => "codex",
            Self::Claude => "claude",
            Self::Abi => "abi",
        }
    }

    fn inventory_command(self) -> ProviderCommand {
        match self {
            // Cursor currently has no `plugin list` leaf. Its structured
            // marketplace view is the honest inventory surface it exposes.
            Self::Cursor => {
                ProviderCommand::new(self, ["plugin", "marketplace", "list", "--format", "json"])
            }
            Self::Codex | Self::Claude => ProviderCommand::new(self, ["plugin", "list", "--json"]),
            Self::Abi => ProviderCommand::new(self, ["plugin", "list"]),
        }
    }
}

pub fn parse_plugin_provider(value: &str) -> Option<PluginProvider> {
    match value.trim().to_ascii_lowercase().as_str() {
        "cursor" | "cursor-agent" => Some(PluginProvider::Cursor),
        "codex" => Some(PluginProvider::Codex),
        "claude" | "claude-code" => Some(PluginProvider::Claude),
        "abi" => Some(PluginProvider::Abi),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginKind {
    Installed,
    Marketplace,
    Bundled,
}

impl PluginKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Installed => "installed",
            Self::Marketplace => "marketplace",
            Self::Bundled => "bundled",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginState {
    Enabled,
    Disabled,
    Installed,
    Visible,
}

impl PluginState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Enabled => "enabled",
            Self::Disabled => "disabled",
            Self::Installed => "installed",
            Self::Visible => "visible",
        }
    }
}

#[derive(Debug, Clone)]
pub struct PluginEntry {
    pub name: String,
    pub provider: PluginProvider,
    pub kind: PluginKind,
    pub state: PluginState,
    pub version: Option<String>,
    pub source: String,
}

#[derive(Debug, Clone)]
pub struct PluginDiagnostic {
    pub provider: PluginProvider,
    pub message: String,
}

#[derive(Debug, Clone, Default)]
pub struct PluginInventory {
    pub entries: Vec<PluginEntry>,
    pub diagnostics: Vec<PluginDiagnostic>,
}

#[derive(Debug, Clone)]
pub struct ProviderCommand {
    pub provider: PluginProvider,
    pub binary: String,
    pub args: Vec<String>,
}

impl ProviderCommand {
    fn new<I, S>(provider: PluginProvider, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            provider,
            binary: provider.binary().into(),
            args: args.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ProviderOutput {
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub timed_out: bool,
}

pub trait PluginRunner {
    fn run(&self, command: &ProviderCommand, timeout: Duration) -> Result<ProviderOutput>;
}

pub struct SystemPluginRunner;

impl PluginRunner for SystemPluginRunner {
    fn run(&self, command: &ProviderCommand, timeout: Duration) -> Result<ProviderOutput> {
        let binary = resolve_provider_binary(command.provider).with_context(|| {
            format!(
                "resolve {} plugin inventory binary",
                command.provider.label()
            )
        })?;
        let mut child = Command::new(binary)
            .args(&command.args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| format!("start {}", command.binary))?;
        let stdout = child.stdout.take().context("capture provider stdout")?;
        let stderr = child.stderr.take().context("capture provider stderr")?;
        let stdout_reader = thread::spawn(move || read_capped(stdout));
        let stderr_reader = thread::spawn(move || read_capped(stderr));
        let started = Instant::now();
        let (status, timed_out) = loop {
            if let Some(status) = child.try_wait()? {
                break (status, false);
            }
            if started.elapsed() >= timeout {
                let _ = child.kill();
                break (child.wait()?, true);
            }
            thread::sleep(Duration::from_millis(10));
        };
        if timed_out {
            // A provider may leave descendants holding inherited pipe handles.
            // Detach the drain threads after killing the direct child so a
            // timeout cannot turn into an unbounded join.
            drop(stdout_reader);
            drop(stderr_reader);
            return Ok(ProviderOutput {
                code: status.code(),
                stdout: String::new(),
                stderr: String::new(),
                timed_out: true,
            });
        }
        let stdout = stdout_reader
            .join()
            .map_err(|_| anyhow::anyhow!("provider stdout reader panicked"))??;
        let stderr = stderr_reader
            .join()
            .map_err(|_| anyhow::anyhow!("provider stderr reader panicked"))??;
        Ok(ProviderOutput {
            code: status.code(),
            stdout,
            stderr,
            timed_out: false,
        })
    }
}

fn resolve_provider_binary(provider: PluginProvider) -> Result<PathBuf> {
    let cfg = crate::config::AbbeyConfig::load().unwrap_or_default();
    resolve_provider_binary_with_config(provider, &cfg)
}

fn resolve_provider_binary_with_config(
    provider: PluginProvider,
    cfg: &crate::config::AbbeyConfig,
) -> Result<PathBuf> {
    if provider == PluginProvider::Abi {
        return crate::config::resolve_abi_bin(cfg).with_context(|| {
            format!(
                "ABI is unavailable; set ABBEY_ABI_BIN or `abi_bin` in {}",
                crate::config::AbbeyConfig::config_path().display()
            )
        });
    }
    crate::agent::which_bin(provider.binary())
        .with_context(|| format!("{} is not on PATH", provider.binary()))
}

fn read_capped(mut reader: impl Read) -> Result<String> {
    let mut retained = Vec::new();
    let mut buffer = [0u8; 8192];
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        let remaining = MAX_CAPTURE_BYTES.saturating_sub(retained.len());
        retained.extend_from_slice(&buffer[..count.min(remaining)]);
    }
    Ok(String::from_utf8_lossy(&retained).into_owned())
}

pub fn inventory_plugins() -> PluginInventory {
    inventory_plugins_with(&PluginProvider::ALL, &SystemPluginRunner, PROVIDER_TIMEOUT)
}

pub fn inventory_plugins_with(
    providers: &[PluginProvider],
    runner: &dyn PluginRunner,
    timeout: Duration,
) -> PluginInventory {
    let mut inventory = PluginInventory::default();
    for provider in providers {
        let command = provider.inventory_command();
        let output = match runner.run(&command, timeout) {
            Ok(output) => output,
            Err(error) => {
                inventory.diagnostics.push(PluginDiagnostic {
                    provider: *provider,
                    message: error.to_string(),
                });
                continue;
            }
        };
        if output.timed_out {
            inventory.diagnostics.push(PluginDiagnostic {
                provider: *provider,
                message: format!("timed out after {}ms", timeout.as_millis()),
            });
            continue;
        }
        if output.code != Some(0) {
            inventory.diagnostics.push(PluginDiagnostic {
                provider: *provider,
                message: nonzero_message(output.code, &output.stderr),
            });
            continue;
        }
        match parse_provider_output(*provider, &output) {
            Ok(mut entries) => inventory.entries.append(&mut entries),
            Err(error) => inventory.diagnostics.push(PluginDiagnostic {
                provider: *provider,
                message: error.to_string(),
            }),
        }
    }
    inventory.entries.sort_by(|left, right| {
        left.provider
            .cmp(&right.provider)
            .then_with(|| left.name.cmp(&right.name))
    });
    inventory.entries.dedup_by(|left, right| {
        left.provider == right.provider
            && left.name == right.name
            && left.kind == right.kind
            && left.version == right.version
    });
    inventory
}

fn nonzero_message(code: Option<i32>, stderr: &str) -> String {
    let detail = stderr.lines().next().unwrap_or("no stderr").trim();
    format!(
        "provider command exited {}: {}",
        code.map_or_else(|| "without a code".into(), |code| code.to_string()),
        redact_message(detail)
    )
}

fn redact_message(message: &str) -> String {
    let lower = message.to_ascii_lowercase();
    if ["token", "secret", "password", "api_key", "apikey"]
        .iter()
        .any(|word| lower.contains(word))
    {
        "provider reported a credential-related error (detail redacted)".into()
    } else {
        message.chars().take(240).collect()
    }
}

fn parse_provider_output(
    provider: PluginProvider,
    output: &ProviderOutput,
) -> Result<Vec<PluginEntry>> {
    match provider {
        PluginProvider::Cursor => parse_cursor_json(&output.stdout),
        PluginProvider::Codex => parse_codex_json(&output.stdout),
        PluginProvider::Claude => parse_claude_json(&output.stdout),
        PluginProvider::Abi => parse_abi_text(if output.stderr.trim().is_empty() {
            &output.stdout
        } else {
            &output.stderr
        }),
    }
}

fn parse_cursor_json(text: &str) -> Result<Vec<PluginEntry>> {
    let values: Vec<Value> = serde_json::from_str(text).context("parse Cursor marketplace JSON")?;
    Ok(values
        .into_iter()
        .filter_map(|value| {
            let name = value.get("name")?.as_str()?.to_string();
            let scope = value
                .get("scope")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            Some(PluginEntry {
                name,
                provider: PluginProvider::Cursor,
                kind: PluginKind::Marketplace,
                state: PluginState::Visible,
                version: None,
                source: format!("cursor marketplace ({scope})"),
            })
        })
        .collect())
}

fn parse_codex_json(text: &str) -> Result<Vec<PluginEntry>> {
    let value: Value = serde_json::from_str(text).context("parse Codex plugin JSON")?;
    let installed = value
        .get("installed")
        .and_then(Value::as_array)
        .context("Codex plugin JSON has no installed array")?;
    Ok(installed
        .iter()
        .filter_map(|value| {
            let name = value
                .get("pluginId")
                .or_else(|| value.get("name"))?
                .as_str()?
                .to_string();
            let enabled = value.get("enabled").and_then(Value::as_bool);
            Some(PluginEntry {
                name,
                provider: PluginProvider::Codex,
                kind: PluginKind::Installed,
                state: match enabled {
                    Some(true) => PluginState::Enabled,
                    Some(false) => PluginState::Disabled,
                    None => PluginState::Installed,
                },
                version: value
                    .get("version")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                source: value
                    .get("marketplaceName")
                    .and_then(Value::as_str)
                    .unwrap_or("codex")
                    .to_string(),
            })
        })
        .collect())
}

fn parse_claude_json(text: &str) -> Result<Vec<PluginEntry>> {
    let values: Vec<Value> = serde_json::from_str(text).context("parse Claude plugin JSON")?;
    Ok(values
        .iter()
        .filter_map(|value| {
            let name = value.get("id")?.as_str()?.to_string();
            let enabled = value.get("enabled").and_then(Value::as_bool);
            Some(PluginEntry {
                name,
                provider: PluginProvider::Claude,
                kind: PluginKind::Installed,
                state: match enabled {
                    Some(true) => PluginState::Enabled,
                    Some(false) => PluginState::Disabled,
                    None => PluginState::Installed,
                },
                version: value
                    .get("version")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                source: value
                    .get("scope")
                    .and_then(Value::as_str)
                    .unwrap_or("claude")
                    .to_string(),
            })
        })
        .collect())
}

fn parse_abi_text(text: &str) -> Result<Vec<PluginEntry>> {
    let entries: Vec<_> = text
        .lines()
        .map(str::trim)
        .filter(|line| {
            !line.is_empty()
                && !line.starts_with("usage:")
                && !line.starts_with("error:")
                && !line.starts_with("warning:")
        })
        .map(|line| PluginEntry {
            name: line.to_string(),
            provider: PluginProvider::Abi,
            kind: PluginKind::Bundled,
            state: PluginState::Enabled,
            version: None,
            source: "abi generated registry".into(),
        })
        .collect();
    if entries.is_empty() {
        bail!("ABI plugin list returned no registry entries");
    }
    Ok(entries)
}

pub fn run_plugin_for_provider(provider: PluginProvider, args: &[String]) -> Result<i32> {
    if args.is_empty() || args == ["list"] {
        let inventory = inventory_plugins_with(&[provider], &SystemPluginRunner, PROVIDER_TIMEOUT);
        for entry in inventory.entries {
            println!(
                "{:<42} [{:<11} {}]",
                entry.name,
                entry.kind.label(),
                entry.state.label()
            );
        }
        if let Some(diagnostic) = inventory.diagnostics.first() {
            eprintln!(
                "{} plugin inventory: {}",
                provider.label(),
                diagnostic.message
            );
            return Ok(2);
        }
        return Ok(0);
    }
    let binary = resolve_provider_binary(provider)?;
    let status = Command::new(binary)
        .arg("plugin")
        .args(args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()?;
    Ok(status.code().unwrap_or(1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::fs;

    struct FakeRunner {
        outputs: BTreeMap<PluginProvider, ProviderOutput>,
    }

    impl PluginRunner for FakeRunner {
        fn run(&self, command: &ProviderCommand, _: Duration) -> Result<ProviderOutput> {
            self.outputs
                .get(&command.provider)
                .cloned()
                .with_context(|| format!("missing fake for {}", command.provider.label()))
        }
    }

    #[test]
    fn parses_provider_structured_outputs_without_cursor_plugin_list() {
        let runner = FakeRunner {
            outputs: BTreeMap::from([
                (
                    PluginProvider::Cursor,
                    ProviderOutput {
                        code: Some(0),
                        stdout: r#"[{"name":"official","scope":"global"}]"#.into(),
                        ..ProviderOutput::default()
                    },
                ),
                (
                    PluginProvider::Codex,
                    ProviderOutput {
                        code: Some(0),
                        stdout:
                            r#"{"installed":[{"pluginId":"p@m","enabled":true,"version":"1"}]}"#
                                .into(),
                        ..ProviderOutput::default()
                    },
                ),
                (
                    PluginProvider::Claude,
                    ProviderOutput {
                        code: Some(0),
                        stdout: r#"[{"id":"q@m","enabled":false,"scope":"user"}]"#.into(),
                        ..ProviderOutput::default()
                    },
                ),
            ]),
        };
        let providers = [
            PluginProvider::Cursor,
            PluginProvider::Codex,
            PluginProvider::Claude,
        ];
        let got = inventory_plugins_with(&providers, &runner, Duration::from_millis(1));
        assert!(got.diagnostics.is_empty());
        assert_eq!(got.entries.len(), 3);
        assert_eq!(got.entries[0].kind, PluginKind::Marketplace);
        assert_eq!(got.entries[1].state, PluginState::Enabled);
        assert_eq!(got.entries[2].state, PluginState::Disabled);
    }

    #[test]
    fn reports_timeout_nonzero_and_malformed_outputs_independently() {
        let runner = FakeRunner {
            outputs: BTreeMap::from([
                (
                    PluginProvider::Cursor,
                    ProviderOutput {
                        timed_out: true,
                        ..ProviderOutput::default()
                    },
                ),
                (
                    PluginProvider::Codex,
                    ProviderOutput {
                        code: Some(7),
                        stderr: "TOKEN=do-not-print".into(),
                        ..ProviderOutput::default()
                    },
                ),
                (
                    PluginProvider::Claude,
                    ProviderOutput {
                        code: Some(0),
                        stdout: "not-json".into(),
                        ..ProviderOutput::default()
                    },
                ),
            ]),
        };
        let got = inventory_plugins_with(
            &[
                PluginProvider::Cursor,
                PluginProvider::Codex,
                PluginProvider::Claude,
            ],
            &runner,
            Duration::from_millis(5),
        );
        assert!(got.entries.is_empty());
        assert_eq!(got.diagnostics.len(), 3);
        assert!(
            got.diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains("timed out"))
        );
        assert!(
            got.diagnostics
                .iter()
                .all(|diagnostic| { !diagnostic.message.contains("do-not-print") })
        );
    }

    #[test]
    fn abi_provider_uses_the_canonical_configured_binary() {
        let configured = std::env::temp_dir().join(format!(
            "abbey-abi-plugin-resolver-{}",
            uuid::Uuid::new_v4()
        ));
        fs::write(&configured, b"test executable placeholder").unwrap();
        let cfg = crate::config::AbbeyConfig {
            abi_bin: Some(configured.clone()),
            ..crate::config::AbbeyConfig::default()
        };

        assert_eq!(
            resolve_provider_binary_with_config(PluginProvider::Abi, &cfg).unwrap(),
            configured
        );
        fs::remove_file(configured).unwrap();
    }
}
