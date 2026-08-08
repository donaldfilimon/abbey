use crate::inventory::plugins::{PluginProvider, PluginRunner, ProviderCommand};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum McpProvider {
    Shared,
    Cursor,
    Codex,
    Claude,
}

impl McpProvider {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "shared" => Some(Self::Shared),
            "cursor" | "cursor-agent" => Some(Self::Cursor),
            "codex" => Some(Self::Codex),
            "claude" | "claude-code" => Some(Self::Claude),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Shared => "shared",
            Self::Cursor => "cursor",
            Self::Codex => "codex",
            Self::Claude => "claude",
        }
    }

    pub fn supports_management(self) -> bool {
        !matches!(self, Self::Shared)
    }

    pub fn binary(self) -> &'static str {
        match self {
            Self::Shared => "",
            Self::Cursor => "cursor-agent",
            Self::Codex => "codex",
            Self::Claude => "claude",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpConfigFormat {
    Json,
    CodexToml,
    ProviderJson,
}

#[derive(Debug, Clone)]
pub struct McpConfigSource {
    pub path: PathBuf,
    pub providers: Vec<McpProvider>,
    pub format: McpConfigFormat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpTransport {
    Stdio,
    Http,
    Sse,
    Unknown,
}

impl McpTransport {
    pub fn label(self) -> &'static str {
        match self {
            Self::Stdio => "stdio",
            Self::Http => "http",
            Self::Sse => "sse",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone)]
pub struct McpServerEntry {
    pub name: String,
    /// Safe executable or redacted URL retained for compatibility.
    pub command: String,
    /// Argument values are never retained; a marker indicates their presence.
    pub args: Vec<String>,
    pub source: PathBuf,
    pub disabled: bool,
    pub transport: McpTransport,
    pub provider: McpProvider,
}

impl McpServerEntry {
    pub fn safe_target(&self) -> &str {
        if self.command.is_empty() {
            "(target not declared)"
        } else {
            &self.command
        }
    }
}

#[derive(Debug, Clone)]
pub struct McpConfigGroup {
    pub source: McpConfigSource,
    pub servers: Vec<McpServerEntry>,
}

#[derive(Debug, Clone)]
pub struct McpDiagnostic {
    pub path: PathBuf,
    pub message: String,
}

#[derive(Debug, Clone, Default)]
pub struct McpInventory {
    pub groups: Vec<McpConfigGroup>,
    pub diagnostics: Vec<McpDiagnostic>,
}

pub fn mcp_config_sources(cwd: &Path) -> Vec<McpConfigSource> {
    mcp_config_sources_with(cwd, dirs::home_dir().as_deref())
}

pub fn mcp_config_sources_with(cwd: &Path, home: Option<&Path>) -> Vec<McpConfigSource> {
    let mut candidates = vec![
        source(
            cwd.join(".mcp.json"),
            &[
                McpProvider::Shared,
                McpProvider::Cursor,
                McpProvider::Claude,
            ],
            McpConfigFormat::Json,
        ),
        source(
            cwd.join(".cursor/mcp.json"),
            &[McpProvider::Cursor],
            McpConfigFormat::Json,
        ),
        source(
            cwd.join(".codex/config.toml"),
            &[McpProvider::Codex],
            McpConfigFormat::CodexToml,
        ),
        source(
            cwd.join(".claude/mcp.json"),
            &[McpProvider::Claude],
            McpConfigFormat::Json,
        ),
    ];
    if let Some(home) = home {
        candidates.extend([
            source(
                home.join(".cursor/mcp.json"),
                &[McpProvider::Cursor],
                McpConfigFormat::Json,
            ),
            source(
                home.join(".codex/config.toml"),
                &[McpProvider::Codex],
                McpConfigFormat::CodexToml,
            ),
            source(
                home.join(".claude/mcp.json"),
                &[McpProvider::Claude],
                McpConfigFormat::Json,
            ),
            source(
                home.join(".claude.json"),
                &[McpProvider::Claude],
                McpConfigFormat::Json,
            ),
            source(
                home.join("Library/Application Support/Claude/claude_desktop_config.json"),
                &[McpProvider::Claude],
                McpConfigFormat::Json,
            ),
            source(
                home.join(".config/claude/mcp.json"),
                &[McpProvider::Claude],
                McpConfigFormat::Json,
            ),
        ]);
    }

    let mut deduped = BTreeMap::<PathBuf, McpConfigSource>::new();
    for candidate in candidates {
        let identity = fs::canonicalize(&candidate.path).unwrap_or_else(|_| candidate.path.clone());
        deduped
            .entry(identity)
            .and_modify(|existing| {
                for provider in &candidate.providers {
                    if !existing.providers.contains(provider) {
                        existing.providers.push(*provider);
                    }
                }
            })
            .or_insert(candidate);
    }
    deduped.into_values().collect()
}

fn source(path: PathBuf, providers: &[McpProvider], format: McpConfigFormat) -> McpConfigSource {
    McpConfigSource {
        path,
        providers: providers.to_vec(),
        format,
    }
}

pub fn load_mcp_inventory(cwd: &Path) -> McpInventory {
    load_mcp_inventory_from(&mcp_config_sources(cwd))
}

pub fn load_mcp_inventory_from(sources: &[McpConfigSource]) -> McpInventory {
    let mut inventory = McpInventory::default();
    for source in sources {
        if !source.path.is_file() {
            continue;
        }
        let text = match fs::read_to_string(&source.path) {
            Ok(text) => text,
            Err(error) => {
                inventory.diagnostics.push(McpDiagnostic {
                    path: source.path.clone(),
                    message: format!("read failed: {error}"),
                });
                continue;
            }
        };
        let servers = match source.format {
            McpConfigFormat::Json | McpConfigFormat::ProviderJson => {
                parse_json_servers(&text, source)
            }
            McpConfigFormat::CodexToml => parse_codex_toml(&text, source),
        };
        match servers {
            Ok(mut servers) => {
                servers.sort_by(|left, right| left.name.cmp(&right.name));
                servers.dedup_by(|left, right| {
                    left.name == right.name
                        && left.transport == right.transport
                        && left.command == right.command
                        && left.disabled == right.disabled
                });
                inventory.groups.push(McpConfigGroup {
                    source: source.clone(),
                    servers,
                });
            }
            Err(message) => inventory.diagnostics.push(McpDiagnostic {
                path: source.path.clone(),
                message,
            }),
        }
    }
    inventory
}

/// Compatibility projection. Malformed files are isolated and omitted; use
/// [`load_mcp_inventory`] when diagnostics are required.
pub fn load_mcp_servers(cwd: &Path) -> anyhow::Result<Vec<(PathBuf, Vec<McpServerEntry>)>> {
    Ok(load_mcp_inventory(cwd)
        .groups
        .into_iter()
        .map(|group| (group.source.path, group.servers))
        .collect())
}

fn parse_json_servers(text: &str, source: &McpConfigSource) -> Result<Vec<McpServerEntry>, String> {
    let value: Value = serde_json::from_str(text).map_err(|error| error.to_string())?;
    if source.format == McpConfigFormat::ProviderJson {
        return parse_codex_provider_json(&value, source);
    }
    let map = value
        .get("mcpServers")
        .or_else(|| value.get("mcp_servers"))
        .and_then(Value::as_object);
    let Some(map) = map else {
        return Ok(Vec::new());
    };
    Ok(map
        .iter()
        .map(|(name, config)| entry_from_json(name, config, source))
        .collect())
}

fn entry_from_json(name: &str, config: &Value, source: &McpConfigSource) -> McpServerEntry {
    let disabled = config
        .get("disabled")
        .and_then(Value::as_bool)
        .unwrap_or_else(|| {
            config
                .get("enabled")
                .and_then(Value::as_bool)
                .is_some_and(|enabled| !enabled)
        });
    let declared_type = config.get("type").and_then(Value::as_str).unwrap_or("");
    let url = config.get("url").and_then(Value::as_str);
    let command = config.get("command").and_then(Value::as_str);
    let transport = classify_transport(declared_type, command, url);
    McpServerEntry {
        name: name.into(),
        command: url.map(redact_url).unwrap_or_else(|| {
            command
                .map(redact_command)
                .unwrap_or_else(|| "(target not declared)".into())
        }),
        args: redacted_args_marker(config.get("args")),
        source: source.path.clone(),
        disabled,
        transport,
        provider: primary_provider(source),
    }
}

fn classify_transport(
    declared_type: &str,
    command: Option<&str>,
    url: Option<&str>,
) -> McpTransport {
    let declared_type = declared_type.to_ascii_lowercase();
    if declared_type.contains("sse") {
        McpTransport::Sse
    } else if declared_type.contains("http") || url.is_some() {
        McpTransport::Http
    } else if declared_type.contains("stdio") || command.is_some() {
        McpTransport::Stdio
    } else {
        McpTransport::Unknown
    }
}

fn primary_provider(source: &McpConfigSource) -> McpProvider {
    source
        .providers
        .iter()
        .copied()
        .find(|provider| *provider != McpProvider::Shared)
        .unwrap_or(McpProvider::Shared)
}

fn redacted_args_marker(value: Option<&Value>) -> Vec<String> {
    match value.and_then(Value::as_array) {
        Some(args) if !args.is_empty() => {
            vec![format!("<{} argument value(s) redacted>", args.len())]
        }
        _ => Vec::new(),
    }
}

fn redact_command(command: &str) -> String {
    if looks_secret(command) {
        "<command redacted>".into()
    } else {
        command.to_string()
    }
}

fn redact_url(url: &str) -> String {
    let without_fragment = url.split('#').next().unwrap_or(url);
    let without_query = without_fragment
        .split('?')
        .next()
        .unwrap_or(without_fragment);
    let Some((scheme, rest)) = without_query.split_once("://") else {
        return if looks_secret(without_query) {
            "<URL redacted>".into()
        } else {
            without_query.into()
        };
    };
    let safe_rest = rest
        .split_once('@')
        .map(|(_, suffix)| format!("<credentials-redacted>@{suffix}"))
        .unwrap_or_else(|| rest.to_string());
    format!("{scheme}://{safe_rest}")
}

fn looks_secret(value: &str) -> bool {
    let value = value.to_ascii_lowercase();
    ["token=", "secret=", "password=", "api_key=", "apikey="]
        .iter()
        .any(|needle| value.contains(needle))
}

#[derive(Default)]
struct TomlServer {
    name: String,
    command: Option<String>,
    url: Option<String>,
    transport_type: Option<String>,
    disabled: bool,
    has_args: bool,
}

fn parse_codex_toml(text: &str, source: &McpConfigSource) -> Result<Vec<McpServerEntry>, String> {
    validate_toml_delimiters(text)?;
    let mut servers = Vec::new();
    let mut current: Option<TomlServer> = None;
    for raw_line in text.lines() {
        let line = strip_toml_comment(raw_line).trim();
        if line.starts_with('[') && line.ends_with(']') {
            if let Some(server) = current.take() {
                servers.push(toml_server_entry(server, source));
            }
            let section = &line[1..line.len() - 1];
            current = codex_server_name(section).map(|name| TomlServer {
                name,
                ..TomlServer::default()
            });
            continue;
        }
        let Some(server) = current.as_mut() else {
            continue;
        };
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim();
        match key {
            "command" => server.command = toml_string(value),
            "url" => server.url = toml_string(value),
            "type" | "transport" => server.transport_type = toml_string(value),
            "enabled" => server.disabled = value.eq_ignore_ascii_case("false"),
            "disabled" => server.disabled = value.eq_ignore_ascii_case("true"),
            "args" => server.has_args = true,
            _ => {}
        }
    }
    if let Some(server) = current {
        servers.push(toml_server_entry(server, source));
    }
    Ok(servers)
}

fn validate_toml_delimiters(text: &str) -> Result<(), String> {
    let mut square = 0isize;
    let mut curly = 0isize;
    let mut quote = None;
    let mut escaped = false;
    for character in text
        .lines()
        .flat_map(|line| strip_toml_comment(line).chars())
    {
        if escaped {
            escaped = false;
            continue;
        }
        if quote == Some('"') && character == '\\' {
            escaped = true;
            continue;
        }
        if matches!(character, '\'' | '"') {
            if quote == Some(character) {
                quote = None;
            } else if quote.is_none() {
                quote = Some(character);
            }
            continue;
        }
        if quote.is_some() {
            continue;
        }
        match character {
            '[' => square += 1,
            ']' => square -= 1,
            '{' => curly += 1,
            '}' => curly -= 1,
            _ => {}
        }
        if square < 0 || curly < 0 {
            return Err("unbalanced TOML delimiters".into());
        }
    }
    if square != 0 || curly != 0 || quote.is_some() {
        Err("unbalanced TOML delimiters or quote".into())
    } else {
        Ok(())
    }
}

fn codex_server_name(section: &str) -> Option<String> {
    let rest = section.strip_prefix("mcp_servers.")?;
    if rest.contains(".env") || rest.contains(".http_headers") {
        return None;
    }
    Some(rest.trim_matches('"').trim_matches('\'').to_string())
}

fn strip_toml_comment(line: &str) -> &str {
    let mut quoted = false;
    for (index, byte) in line.bytes().enumerate() {
        if byte == b'"' {
            quoted = !quoted;
        } else if byte == b'#' && !quoted {
            return &line[..index];
        }
    }
    line
}

fn toml_string(value: &str) -> Option<String> {
    let value = value.trim();
    value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .or_else(|| {
            value
                .strip_prefix('\'')
                .and_then(|value| value.strip_suffix('\''))
        })
        .map(str::to_string)
}

fn toml_server_entry(server: TomlServer, source: &McpConfigSource) -> McpServerEntry {
    let transport = classify_transport(
        server.transport_type.as_deref().unwrap_or(""),
        server.command.as_deref(),
        server.url.as_deref(),
    );
    McpServerEntry {
        name: server.name,
        command: server
            .url
            .as_deref()
            .map(redact_url)
            .or_else(|| server.command.as_deref().map(redact_command))
            .unwrap_or_else(|| "(target not declared)".into()),
        args: if server.has_args {
            vec!["<argument values redacted>".into()]
        } else {
            Vec::new()
        },
        source: source.path.clone(),
        disabled: server.disabled,
        transport,
        provider: McpProvider::Codex,
    }
}

pub fn provider_mcp_view_with(
    provider: McpProvider,
    runner: &dyn PluginRunner,
    timeout: Duration,
) -> McpInventory {
    let mut inventory = McpInventory::default();
    if provider != McpProvider::Codex {
        inventory.diagnostics.push(McpDiagnostic {
            path: PathBuf::from(format!("<{} provider>", provider.label())),
            message:
                "provider has no authoritative structured MCP list output; use config inventory"
                    .into(),
        });
        return inventory;
    }
    let command = ProviderCommand {
        provider: PluginProvider::Codex,
        binary: "codex".into(),
        args: vec!["mcp".into(), "list".into(), "--json".into()],
    };
    let output = match runner.run(&command, timeout) {
        Ok(output) => output,
        Err(error) => {
            inventory.diagnostics.push(McpDiagnostic {
                path: PathBuf::from("<codex provider>"),
                message: error.to_string(),
            });
            return inventory;
        }
    };
    if output.timed_out || output.code != Some(0) {
        inventory.diagnostics.push(McpDiagnostic {
            path: PathBuf::from("<codex provider>"),
            message: if output.timed_out {
                format!("timed out after {}ms", timeout.as_millis())
            } else {
                format!(
                    "provider command exited {}",
                    output
                        .code
                        .map_or_else(|| "without a code".into(), |code| code.to_string())
                )
            },
        });
        return inventory;
    }
    let source = McpConfigSource {
        path: PathBuf::from("<codex provider view>"),
        providers: vec![McpProvider::Codex],
        format: McpConfigFormat::ProviderJson,
    };
    match parse_json_servers(&output.stdout, &source) {
        Ok(servers) => inventory.groups.push(McpConfigGroup { source, servers }),
        Err(message) => inventory.diagnostics.push(McpDiagnostic {
            path: PathBuf::from("<codex provider>"),
            message,
        }),
    }
    inventory
}

fn parse_codex_provider_json(
    value: &Value,
    source: &McpConfigSource,
) -> Result<Vec<McpServerEntry>, String> {
    let values = value
        .as_array()
        .ok_or_else(|| "Codex MCP JSON is not an array".to_string())?;
    Ok(values
        .iter()
        .filter_map(|value| {
            let name = value.get("name")?.as_str()?;
            let enabled = value
                .get("enabled")
                .and_then(Value::as_bool)
                .unwrap_or(true);
            let transport = value.get("transport").unwrap_or(&Value::Null);
            let kind = transport.get("type").and_then(Value::as_str).unwrap_or("");
            let command = transport.get("command").and_then(Value::as_str);
            let url = transport.get("url").and_then(Value::as_str);
            Some(McpServerEntry {
                name: name.into(),
                command: url.map(redact_url).unwrap_or_else(|| {
                    command
                        .map(redact_command)
                        .unwrap_or_else(|| "(target not declared)".into())
                }),
                args: redacted_args_marker(transport.get("args")),
                source: source.path.clone(),
                disabled: !enabled,
                transport: classify_transport(kind, command, url),
                provider: McpProvider::Codex,
            })
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inventory::plugins::{ProviderCommand, ProviderOutput};
    use anyhow::Result;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root(label: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("abbey-mcp-{label}-{stamp}"));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn injectable_roots_dedup_sources_and_isolate_malformed_files() {
        let root = temp_root("roots");
        fs::create_dir_all(root.join(".cursor")).unwrap();
        fs::create_dir_all(root.join(".codex")).unwrap();
        fs::write(
            root.join(".cursor/mcp.json"),
            r#"{"mcpServers":{"safe":{"command":"npx","args":["--token","secret"]}}}"#,
        )
        .unwrap();
        fs::write(root.join(".codex/config.toml"), "not = [ valid").unwrap();
        let sources = mcp_config_sources_with(&root, Some(&root));
        let cursor_count = sources
            .iter()
            .filter(|source| source.path == root.join(".cursor/mcp.json"))
            .count();
        assert_eq!(cursor_count, 1);

        let inventory = load_mcp_inventory_from(&sources);
        let server = inventory
            .groups
            .iter()
            .flat_map(|group| &group.servers)
            .find(|server| server.name == "safe")
            .unwrap();
        assert_eq!(server.transport, McpTransport::Stdio);
        assert_eq!(server.args, vec!["<2 argument value(s) redacted>"]);
        assert!(!format!("{server:?}").contains("secret"));
        assert_eq!(inventory.diagnostics.len(), 1);
        fs::write(root.join(".codex/config.toml"), "[[[").unwrap();
        // One malformed source does not prevent the valid Cursor file loading.
        let still_valid = load_mcp_inventory_from(&sources);
        assert!(
            still_valid
                .groups
                .iter()
                .any(|group| { group.servers.iter().any(|server| server.name == "safe") })
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn parses_stdio_http_sse_and_configured_disabled_state() {
        let source = source(
            PathBuf::from("/tmp/mcp.json"),
            &[McpProvider::Cursor],
            McpConfigFormat::Json,
        );
        let servers = parse_json_servers(
            r#"{"mcpServers":{
                "stdio":{"command":"npx","args":["--api-key=hidden"]},
                "http":{"type":"http","url":"https://user:pass@example.test/mcp?token=hidden"},
                "sse":{"type":"sse","url":"https://example.test/sse","disabled":true}
            }}"#,
            &source,
        )
        .unwrap();
        let http = servers.iter().find(|server| server.name == "http").unwrap();
        let sse = servers.iter().find(|server| server.name == "sse").unwrap();
        let stdio = servers
            .iter()
            .find(|server| server.name == "stdio")
            .unwrap();
        assert_eq!(http.transport, McpTransport::Http);
        assert_eq!(
            http.command,
            "https://<credentials-redacted>@example.test/mcp"
        );
        assert_eq!(sse.transport, McpTransport::Sse);
        assert!(sse.disabled);
        assert_eq!(stdio.transport, McpTransport::Stdio);
        assert!(!format!("{servers:?}").contains("hidden"));
    }

    struct FakeRunner(ProviderOutput);

    impl PluginRunner for FakeRunner {
        fn run(&self, _: &ProviderCommand, _: Duration) -> Result<ProviderOutput> {
            Ok(self.0.clone())
        }
    }

    #[test]
    fn codex_provider_view_uses_structured_output_and_redacts_args() {
        let runner = FakeRunner(ProviderOutput {
            code: Some(0),
            stdout: r#"[{"name":"local","enabled":false,"transport":{"type":"stdio","command":"npx","args":["TOKEN=hidden"]}},{"name":"remote","enabled":true,"transport":{"type":"streamable_http","url":"https://example.test/mcp?key=hidden"}}]"#.into(),
            ..ProviderOutput::default()
        });
        let got = provider_mcp_view_with(McpProvider::Codex, &runner, Duration::from_millis(5));
        assert!(got.diagnostics.is_empty());
        let servers = &got.groups[0].servers;
        assert!(servers[0].disabled);
        assert_eq!(servers[0].transport, McpTransport::Stdio);
        assert_eq!(servers[1].transport, McpTransport::Http);
        assert!(!format!("{servers:?}").contains("hidden"));
    }
}
