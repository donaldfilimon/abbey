//! Configured learned-embedding providers.
//!
//! Provider selection is explicit. `none` is a real disabled provider and an
//! unavailable Apple/OpenAI provider is an error; Abbey never substitutes the
//! lexical feature hash or another remote service behind the user's back.

use crate::config::EmbeddingConfig;
use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use url::{Host, Url};

pub const MAX_EMBEDDING_BATCH: usize = 64;
pub const MAX_EMBEDDING_DIMENSION: usize = 4096;
const MAX_INPUT_CHARS: usize = 1_000_000;
const MAX_PROVIDER_STDOUT: usize = 16 * 1024 * 1024;
const MAX_PROVIDER_STDERR: usize = 64 * 1024;
const PROVIDER_TIMEOUT: Duration = Duration::from_secs(30);
pub const NORMALIZATION: &str = "l2-v1";

/// Stable identity for vectors that may be compared with each other.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmbeddingSpace {
    pub space_id: String,
    pub provider: String,
    pub model: String,
    pub revision: String,
    pub dimension: usize,
    pub normalization: String,
}

impl EmbeddingSpace {
    pub fn new(
        provider: impl Into<String>,
        model: impl Into<String>,
        revision: impl Into<String>,
        dimension: usize,
    ) -> Result<Self> {
        let provider = provider.into();
        let model = model.into();
        let revision = revision.into();
        if provider.trim().is_empty() || model.trim().is_empty() || revision.trim().is_empty() {
            bail!("embedding space provider/model/revision must not be empty");
        }
        if provider != "none" && !(1..=MAX_EMBEDDING_DIMENSION).contains(&dimension) {
            bail!("embedding dimension must be between 1 and {MAX_EMBEDDING_DIMENSION}");
        }
        let normalization = NORMALIZATION.to_string();
        let identity = format!(
            "provider={provider}\nmodel={model}\nrevision={revision}\ndimension={dimension}\nnormalization={normalization}"
        );
        let space_id = format!("sem-v1-{}", stable_digest(identity.as_bytes()));
        Ok(Self {
            space_id,
            provider,
            model,
            revision,
            dimension,
            normalization,
        })
    }
}

/// Object-safe learned embedding provider.
pub trait Embedder: Send + Sync {
    fn space(&self) -> &EmbeddingSpace;
    fn embed(&self, inputs: &[String]) -> Result<Vec<Vec<f32>>>;
}

pub struct NoneEmbedder {
    space: EmbeddingSpace,
}

impl NoneEmbedder {
    fn new() -> Result<Self> {
        Ok(Self {
            space: EmbeddingSpace::new("none", "disabled", "v1", 0)?,
        })
    }
}

impl Embedder for NoneEmbedder {
    fn space(&self) -> &EmbeddingSpace {
        &self.space
    }

    fn embed(&self, _inputs: &[String]) -> Result<Vec<Vec<f32>>> {
        bail!("semantic embeddings are disabled (embedding provider is `none`)")
    }
}

pub struct AppleEmbedder {
    space: EmbeddingSpace,
    language: String,
    helper: PathBuf,
}

impl AppleEmbedder {
    fn new(config: &EmbeddingConfig) -> Result<Self> {
        if !cfg!(target_os = "macos") {
            bail!("embedding provider `apple` requires macOS NaturalLanguage");
        }
        let model = format!(
            "{}:{}",
            required(&config.model, "embedding model")?,
            config.language
        );
        Ok(Self {
            space: EmbeddingSpace::new(
                "apple",
                model,
                "natural-language-sentence-v1",
                config.dimension,
            )?,
            language: required(&config.language, "embedding language")?.to_string(),
            helper: PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("scripts")
                .join("abbey-embedding.swift"),
        })
    }
}

impl Embedder for AppleEmbedder {
    fn space(&self) -> &EmbeddingSpace {
        &self.space
    }

    fn embed(&self, inputs: &[String]) -> Result<Vec<Vec<f32>>> {
        validate_batch(inputs)?;
        if inputs.is_empty() {
            return Ok(Vec::new());
        }
        if !self.helper.is_file() {
            bail!(
                "Apple embedding helper is missing: {}",
                self.helper.display()
            );
        }
        let request = serde_json::to_vec(&ProviderRequest {
            language: Some(self.language.clone()),
            model: None,
            dimensions: None,
            encoding_format: None,
            input: inputs.to_vec(),
        })?;
        let output = run_bounded(
            Command::new("/usr/bin/swift")
                .arg(&self.helper)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped()),
            &request,
            PROVIDER_TIMEOUT,
        )
        .context("run Apple NaturalLanguage embedding helper")?;
        parse_provider_response(&output, inputs.len(), self.space.dimension)
    }
}

pub struct OpenAiEmbedder {
    space: EmbeddingSpace,
    url: String,
    api_key: String,
}

impl OpenAiEmbedder {
    fn new(config: &EmbeddingConfig) -> Result<Self> {
        let endpoint = required(&config.endpoint, "embedding endpoint")?;
        let url = embeddings_url(endpoint)?;
        let api_key = embedding_api_key(|name| std::env::var(name).ok())?;
        Self::with_key(config, url, api_key)
    }

    fn with_key(config: &EmbeddingConfig, url: String, api_key: String) -> Result<Self> {
        if api_key.trim().is_empty() {
            bail!("OpenAI-compatible embedding API key must not be empty");
        }
        let model = required(&config.model, "embedding model")?.to_string();
        Ok(Self {
            space: EmbeddingSpace::new("openai", &model, "openai-embeddings-v1", config.dimension)?,
            url,
            api_key,
        })
    }

    fn command(&self, header: &SecretHeader) -> Command {
        let parsed_url =
            Url::parse(&self.url).expect("embedding URL was validated at construction");
        let loopback = is_loopback_http(&parsed_url);
        let protocol = if loopback { "=http" } else { "=https" };
        let mut command = Command::new("curl");
        command
            .args([
                "--silent",
                "--show-error",
                "--fail-with-body",
                "--proto",
                protocol,
                "--connect-timeout",
                "5",
                "--max-time",
                "30",
                "--max-filesize",
                "16777216",
                "--header",
                "Content-Type: application/json",
                "--header",
            ])
            // curl's @file header form keeps the bearer token out of argv.
            .arg(format!("@{}", header.path().display()))
            .args(["--data-binary", "@-", &self.url])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if !loopback {
            command.args(["--tlsv1.2", "--proto-redir", "=https"]);
        }
        command
    }
}

impl Embedder for OpenAiEmbedder {
    fn space(&self) -> &EmbeddingSpace {
        &self.space
    }

    fn embed(&self, inputs: &[String]) -> Result<Vec<Vec<f32>>> {
        validate_batch(inputs)?;
        if inputs.is_empty() {
            return Ok(Vec::new());
        }
        let request = serde_json::to_vec(&ProviderRequest {
            language: None,
            model: Some(self.space.model.clone()),
            dimensions: Some(self.space.dimension),
            encoding_format: Some("float"),
            input: inputs.to_vec(),
        })?;
        let header = SecretHeader::new(&self.api_key)?;
        let mut command = self.command(&header);
        let output = run_bounded(&mut command, &request, PROVIDER_TIMEOUT)
            .context("call OpenAI-compatible embeddings endpoint")?;
        parse_provider_response(&output, inputs.len(), self.space.dimension)
    }
}

/// Permission-restricted bearer-header material removed on every return path.
struct SecretHeader {
    path: PathBuf,
}

impl SecretHeader {
    fn new(api_key: &str) -> Result<Self> {
        let path = std::env::temp_dir().join(format!(
            "abbey-embedding-header-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        let header = Self { path };
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(header.path()).with_context(|| {
            format!(
                "create temporary embedding auth header {}",
                header.path().display()
            )
        })?;
        writeln!(file, "Authorization: Bearer {api_key}")
            .context("write temporary embedding auth header")?;
        file.sync_all()
            .context("flush temporary embedding auth header")?;
        Ok(header)
    }

    fn path(&self) -> &std::path::Path {
        &self.path
    }
}

impl Drop for SecretHeader {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Build exactly the configured provider. There is deliberately no fallback.
pub fn build_embedder(config: &EmbeddingConfig) -> Result<Box<dyn Embedder>> {
    match config.provider.trim().to_ascii_lowercase().as_str() {
        "none" => Ok(Box::new(NoneEmbedder::new()?)),
        "apple" => Ok(Box::new(AppleEmbedder::new(config)?)),
        "openai" => Ok(Box::new(OpenAiEmbedder::new(config)?)),
        other => bail!("unknown embedding provider {other:?}; expected none, apple, or openai"),
    }
}

fn required<'a>(value: &'a str, label: &str) -> Result<&'a str> {
    let value = value.trim();
    if value.is_empty() {
        bail!("{label} must not be empty");
    }
    Ok(value)
}

/// Abbey's own credential variable is edition-scoped (safe:
/// `ABBEY_EMBEDDING_API_KEY`), so one exported Abbey key never reaches the
/// other edition. `OPENAI_API_KEY` stays a shared, provider-owned fallback —
/// it is the user's provider credential, not an Abbey namespace.
fn embedding_api_key(get: impl Fn(&str) -> Option<String>) -> Result<String> {
    let scoped = crate::edition::ACTIVE.credential_env("EMBEDDING_API_KEY");
    get(&scoped)
        .or_else(|| get("OPENAI_API_KEY"))
        .filter(|key| !key.trim().is_empty())
        .ok_or_else(|| anyhow!("embedding provider `openai` needs {scoped} or OPENAI_API_KEY"))
}

fn embeddings_url(endpoint: &str) -> Result<String> {
    let endpoint = endpoint.trim();
    if endpoint.is_empty() {
        bail!("embedding endpoint must not be empty");
    }
    let mut parsed = Url::parse(endpoint).context("parse embedding endpoint URL")?;
    if !parsed.username().is_empty() || parsed.password().is_some() {
        bail!("embedding endpoint must not contain URL userinfo");
    }
    if parsed.query().is_some() || parsed.fragment().is_some() {
        bail!("embedding endpoint must not contain a query or fragment");
    }
    if parsed.scheme() != "https" && !is_loopback_http(&parsed) {
        bail!(
            "OpenAI-compatible embedding endpoint must use HTTPS (HTTP is allowed only for loopback tests)"
        );
    }
    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        bail!("embedding endpoint must use HTTP or HTTPS");
    }
    let path = parsed.path().trim_end_matches('/');
    if !path.ends_with("/v1/embeddings") {
        let target = if path.ends_with("/v1") {
            format!("{path}/embeddings")
        } else {
            format!("{path}/v1/embeddings")
        };
        parsed.set_path(&target);
    }
    Ok(parsed.to_string())
}

fn is_loopback_http(url: &Url) -> bool {
    if url.scheme() != "http" {
        return false;
    }
    match url.host() {
        Some(Host::Domain(host)) => host.eq_ignore_ascii_case("localhost"),
        Some(Host::Ipv4(address)) => address.is_loopback(),
        Some(Host::Ipv6(address)) => address.is_loopback(),
        None => false,
    }
}

fn validate_batch(inputs: &[String]) -> Result<()> {
    if inputs.len() > MAX_EMBEDDING_BATCH {
        bail!(
            "embedding batch has {} inputs; maximum is {MAX_EMBEDDING_BATCH}",
            inputs.len()
        );
    }
    let chars = inputs
        .iter()
        .map(|input| input.chars().count())
        .sum::<usize>();
    if chars > MAX_INPUT_CHARS {
        bail!("embedding batch is too large ({chars} chars; maximum is {MAX_INPUT_CHARS})");
    }
    Ok(())
}

fn run_bounded(command: &mut Command, input: &[u8], timeout: Duration) -> Result<Vec<u8>> {
    let mut child = command.spawn().context("spawn embedding provider")?;
    let mut stdin = child
        .stdin
        .take()
        .context("embedding provider stdin unavailable")?;
    stdin.write_all(input)?;
    drop(stdin);
    // Drain both pipes concurrently: a normal 64 x 1536 JSON response is much
    // larger than a platform pipe buffer and must not deadlock before wait().
    let mut stdout = child
        .stdout
        .take()
        .context("embedding provider stdout unavailable")?;
    let mut stderr = child
        .stderr
        .take()
        .context("embedding provider stderr unavailable")?;
    let stdout_reader = std::thread::spawn(move || drain_bounded(&mut stdout, MAX_PROVIDER_STDOUT));
    let stderr_reader = std::thread::spawn(move || drain_bounded(&mut stderr, MAX_PROVIDER_STDERR));
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait()? {
            let stdout = stdout_reader
                .join()
                .map_err(|_| anyhow!("embedding stdout reader panicked"))??;
            let stderr = stderr_reader
                .join()
                .map_err(|_| anyhow!("embedding stderr reader panicked"))??;
            if stdout.truncated {
                bail!("embedding provider response exceeded {MAX_PROVIDER_STDOUT} bytes");
            }
            if !status.success() {
                let error = String::from_utf8_lossy(&stderr.bytes);
                let body = String::from_utf8_lossy(&stdout.bytes);
                bail!(
                    "embedding provider failed ({}): {}{}{}",
                    status,
                    error.trim(),
                    if stderr.truncated {
                        " [stderr truncated]"
                    } else {
                        ""
                    },
                    if body.trim().is_empty() {
                        String::new()
                    } else {
                        format!("; response: {}", body.trim())
                    }
                );
            }
            return Ok(stdout.bytes);
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            bail!("embedding provider timed out after {}s", timeout.as_secs());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

struct BoundedOutput {
    bytes: Vec<u8>,
    truncated: bool,
}

/// Drain to EOF so the child cannot block on a full pipe, while retaining only
/// a bounded prefix in memory.
fn drain_bounded(reader: &mut impl Read, limit: usize) -> std::io::Result<BoundedOutput> {
    let mut bytes = Vec::with_capacity(limit.min(8192));
    let mut truncated = false;
    let mut buffer = [0_u8; 8192];
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        let remaining = limit.saturating_sub(bytes.len());
        let keep = remaining.min(count);
        bytes.extend_from_slice(&buffer[..keep]);
        truncated |= keep < count;
    }
    Ok(BoundedOutput { bytes, truncated })
}

#[derive(Serialize)]
struct ProviderRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    language: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    dimensions: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    encoding_format: Option<&'static str>,
    input: Vec<String>,
}

#[derive(Deserialize)]
struct ProviderResponse {
    data: Vec<ProviderEmbedding>,
}

#[derive(Deserialize)]
struct ProviderEmbedding {
    index: usize,
    embedding: Vec<f32>,
}

fn parse_provider_response(bytes: &[u8], count: usize, dimension: usize) -> Result<Vec<Vec<f32>>> {
    let response: ProviderResponse = serde_json::from_slice(bytes).with_context(|| {
        format!(
            "parse embedding response: {}",
            String::from_utf8_lossy(bytes)
        )
    })?;
    if response.data.len() != count {
        bail!(
            "embedding provider returned {} vectors for {count} inputs",
            response.data.len()
        );
    }
    let mut ordered = vec![None; count];
    for item in response.data {
        if item.index >= count || ordered[item.index].is_some() {
            bail!(
                "embedding provider returned an invalid/duplicate index {}",
                item.index
            );
        }
        if item.embedding.len() != dimension {
            bail!(
                "embedding provider returned dimension {}; configured dimension is {dimension}",
                item.embedding.len()
            );
        }
        if !item.embedding.iter().all(|value| value.is_finite()) {
            bail!("embedding provider returned a non-finite vector");
        }
        ordered[item.index] = Some(normalize(item.embedding)?);
    }
    ordered
        .into_iter()
        .map(|item| item.context("embedding provider omitted an input index"))
        .collect()
}

pub(crate) fn normalize(mut vector: Vec<f32>) -> Result<Vec<f32>> {
    let norm = vector
        .iter()
        .map(|value| f64::from(*value) * f64::from(*value))
        .sum::<f64>()
        .sqrt();
    if !norm.is_finite() || norm <= f64::EPSILON {
        bail!("embedding vector has zero or invalid norm");
    }
    for value in &mut vector {
        *value = (f64::from(*value) / norm) as f32;
    }
    Ok(vector)
}

/// Dependency-free stable digest for namespace/content identity (two seeded FNV-1a lanes).
pub(crate) fn stable_digest(bytes: &[u8]) -> String {
    fn lane(bytes: &[u8], seed: u64) -> u64 {
        bytes.iter().fold(seed, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3)
        })
    }
    format!(
        "{:016x}{:016x}",
        lane(bytes, 0xcbf2_9ce4_8422_2325),
        lane(bytes, 0x8422_2325_cbf2_9ce4)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    fn config(provider: &str, endpoint: String, dimension: usize) -> EmbeddingConfig {
        EmbeddingConfig {
            provider: provider.into(),
            endpoint,
            model: "mock-model".into(),
            dimension,
            language: "en".into(),
        }
    }

    fn mock_server(status: &str, body: &'static str) -> (String, thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let status = status.to_string();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            let mut buffer = [0_u8; 4096];
            loop {
                let count = stream.read(&mut buffer).unwrap();
                request.extend_from_slice(&buffer[..count]);
                let header_end = request.windows(4).position(|part| part == b"\r\n\r\n");
                let expected = header_end.and_then(|end| {
                    let headers = String::from_utf8_lossy(&request[..end]);
                    headers
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .and_then(|value| value.trim().parse::<usize>().ok())
                        })
                        .map(|length| end + 4 + length)
                });
                if expected.is_some_and(|expected| request.len() >= expected) || count == 0 {
                    break;
                }
            }
            let response = format!(
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(response.as_bytes()).unwrap();
            String::from_utf8(request).unwrap()
        });
        (format!("http://{address}"), handle)
    }

    #[test]
    fn spaces_isolate_model_revision_and_dimensions() {
        let a = EmbeddingSpace::new("openai", "a", "r1", 3).unwrap();
        let b = EmbeddingSpace::new("openai", "b", "r1", 3).unwrap();
        let c = EmbeddingSpace::new("openai", "a", "r2", 3).unwrap();
        let d = EmbeddingSpace::new("openai", "a", "r1", 4).unwrap();
        assert_ne!(a.space_id, b.space_id);
        assert_ne!(a.space_id, c.space_id);
        assert_ne!(a.space_id, d.space_id);
        assert_eq!(a, EmbeddingSpace::new("openai", "a", "r1", 3).unwrap());
    }

    #[test]
    fn openai_request_uses_v1_contract_auth_and_order() {
        let (endpoint, handle) = mock_server(
            "200 OK",
            r#"{"data":[{"index":1,"embedding":[0,2]},{"index":0,"embedding":[3,0]}]}"#,
        );
        let cfg = config("openai", endpoint.clone(), 2);
        let provider = OpenAiEmbedder::with_key(
            &cfg,
            embeddings_url(&endpoint).unwrap(),
            "test-secret".into(),
        )
        .unwrap();
        let vectors = provider.embed(&["first".into(), "second".into()]).unwrap();
        assert_eq!(vectors, vec![vec![1.0, 0.0], vec![0.0, 1.0]]);
        let request = handle.join().unwrap();
        assert!(request.starts_with("POST /v1/embeddings HTTP/1.1"));
        assert!(request.contains("Authorization: Bearer test-secret"));
        assert!(request.contains(r#""model":"mock-model""#));
        assert!(request.contains(r#""dimensions":2"#));
        assert!(request.contains(r#""encoding_format":"float""#));
        assert!(request.contains(r#""input":["first","second"]"#));
    }

    #[test]
    fn openai_secret_is_not_in_argv_and_header_file_is_ephemeral() {
        use std::ffi::OsStr;

        let cfg = config("openai", "http://127.0.0.1:9".into(), 2);
        let provider = OpenAiEmbedder::with_key(
            &cfg,
            embeddings_url(&cfg.endpoint).unwrap(),
            "argv-must-not-contain-this-secret".into(),
        )
        .unwrap();
        let header = SecretHeader::new(&provider.api_key).unwrap();
        let path = header.path().to_path_buf();
        let command = provider.command(&header);
        assert!(command.get_args().all(|argument| {
            argument != OsStr::new("argv-must-not-contain-this-secret")
                && !argument
                    .to_string_lossy()
                    .contains("argv-must-not-contain-this-secret")
        }));
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "Authorization: Bearer argv-must-not-contain-this-secret\n"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        drop(header);
        assert!(!path.exists(), "secret header must be deleted on drop");
    }

    #[test]
    fn pipe_drains_bound_retained_bytes() {
        let input = vec![b'x'; 1024];
        let output = drain_bounded(&mut input.as_slice(), 17).unwrap();
        assert_eq!(output.bytes.len(), 17);
        assert!(output.truncated);
    }

    #[test]
    fn openai_http_errors_are_errors_not_empty_vectors() {
        let (endpoint, handle) = mock_server("401 Unauthorized", r#"{"error":"denied"}"#);
        let cfg = config("openai", endpoint.clone(), 2);
        let provider =
            OpenAiEmbedder::with_key(&cfg, embeddings_url(&endpoint).unwrap(), "bad-key".into())
                .unwrap();
        let error = provider.embed(&["first".into()]).unwrap_err();
        assert!(format!("{error:#}").contains("401"));
        let _ = handle.join().unwrap();

        let (endpoint, handle) = mock_server("429 Too Many Requests", r#"{"error":"rate"}"#);
        let cfg = config("openai", endpoint.clone(), 2);
        let provider =
            OpenAiEmbedder::with_key(&cfg, embeddings_url(&endpoint).unwrap(), "key".into())
                .unwrap();
        let error = provider.embed(&["first".into()]).unwrap_err();
        assert!(format!("{error:#}").contains("429"));
        let _ = handle.join().unwrap();
    }

    #[test]
    fn openai_missing_key_and_malformed_response_fail_without_fallback() {
        assert!(embedding_api_key(|_| None).is_err());
        let (endpoint, handle) = mock_server("200 OK", r#"{"data":"not-an-array"}"#);
        let cfg = config("openai", endpoint.clone(), 2);
        let provider =
            OpenAiEmbedder::with_key(&cfg, embeddings_url(&endpoint).unwrap(), "key".into())
                .unwrap();
        let error = provider.embed(&["first".into()]).unwrap_err();
        assert!(format!("{error:#}").contains("parse embedding response"));
        let _ = handle.join().unwrap();
    }

    #[test]
    fn configured_provider_never_falls_back() {
        let mut cfg = config("made-up", "https://example.invalid".into(), 2);
        assert!(build_embedder(&cfg).is_err());
        cfg.provider = "none".into();
        let none = build_embedder(&cfg).unwrap();
        assert!(none.embed(&["text".into()]).is_err());
    }

    #[test]
    fn remote_plain_http_is_rejected() {
        assert!(embeddings_url("http://example.com").is_err());
        assert!(embeddings_url("http://127.0.0.1:1234").is_ok());
        assert!(embeddings_url("http://127.0.0.1:80@example.test").is_err());
        assert!(embeddings_url("https://user:pass@example.test").is_err());
        assert!(embeddings_url("https://example.test?token=secret").is_err());
    }

    #[test]
    fn endpoint_paths_and_dimensions_are_normalized_and_bounded() {
        assert_eq!(
            embeddings_url("https://example.test").unwrap(),
            "https://example.test/v1/embeddings"
        );
        assert_eq!(
            embeddings_url("https://example.test/v1").unwrap(),
            "https://example.test/v1/embeddings"
        );
        assert_eq!(
            embeddings_url("https://example.test/api/v1").unwrap(),
            "https://example.test/api/v1/embeddings"
        );
        assert_eq!(
            embeddings_url("https://example.test/v1/embeddings").unwrap(),
            "https://example.test/v1/embeddings"
        );
        assert!(EmbeddingSpace::new("openai", "model", "r1", 4096).is_ok());
        assert!(EmbeddingSpace::new("openai", "model", "r1", 4097).is_err());
    }
}
