//! Minimal, bounded HTTP/1.1 request reading and response writing.
//!
//! This is not a general HTTP server. It parses exactly the shape MCP's
//! Streamable HTTP transport needs and refuses everything else explicitly
//! rather than guessing — chunked bodies get a `501`, not a mis-parse.
//!
//! ## One request per connection
//!
//! Every response carries `Connection: close` and the socket is dropped
//! afterwards. That is a deliberate simplification: with no keep-alive there is
//! no half-read body to carry between requests, and a concurrency permit taken
//! at `accept` covers exactly one request. Pipelining and SSE streams are not
//! implemented.

use std::io::{BufRead, ErrorKind, Read, Write};
use std::time::{Duration, Instant};

use crate::mcp_host::limits::{MAX_HTTP_HEAD_BYTES, MAX_HTTP_HEADERS, MAX_REQUEST_BYTES};

/// A parsed request. Header names are lowercased; values keep their case.
pub struct HttpRequest {
    pub method: String,
    pub path: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl HttpRequest {
    /// First value for `name`, which must already be lowercase.
    #[must_use]
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }
}

/// An HTTP-level rejection, carrying the status the peer should see.
#[derive(Debug, Clone)]
pub struct HttpError {
    pub status: u16,
    pub message: String,
}

impl HttpError {
    pub fn new(status: u16, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }
}

/// Outcome of reading a request: a request, a rejection worth answering, or a
/// peer that went away before saying anything.
pub enum ReadOutcome {
    Request(Box<HttpRequest>),
    Rejected(HttpError),
    Disconnected,
}

/// Reason phrase for the small set of statuses this transport emits.
#[must_use]
pub fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        202 => "Accepted",
        204 => "No Content",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        408 => "Request Timeout",
        413 => "Payload Too Large",
        415 => "Unsupported Media Type",
        429 => "Too Many Requests",
        431 => "Request Header Fields Too Large",
        501 => "Not Implemented",
        503 => "Service Unavailable",
        505 => "HTTP Version Not Supported",
        _ => "Error",
    }
}

/// Read one request, bounded by `MAX_HTTP_HEAD_BYTES`, `MAX_HTTP_HEADERS`,
/// `MAX_REQUEST_BYTES`, and an overall `deadline`.
///
/// The deadline spans the whole read rather than each individual `read` call,
/// so a peer that trickles a byte at a time cannot keep resetting the clock.
pub fn read_request(reader: &mut impl BufRead, deadline: Instant) -> ReadOutcome {
    let mut head = Vec::new();
    let mut lines: Vec<String> = Vec::new();
    loop {
        if Instant::now() >= deadline {
            return ReadOutcome::Rejected(HttpError::new(
                408,
                "timed out reading the request head",
            ));
        }
        let mut line = Vec::new();
        let remaining = MAX_HTTP_HEAD_BYTES.saturating_sub(head.len()) + 1;
        let read = match reader
            .by_ref()
            .take(remaining as u64)
            .read_until(b'\n', &mut line)
        {
            Ok(read) => read,
            Err(error) if is_timeout(&error) => {
                return ReadOutcome::Rejected(HttpError::new(
                    408,
                    "timed out reading the request head",
                ));
            }
            Err(_) => return ReadOutcome::Disconnected,
        };
        if read == 0 {
            // EOF before any bytes at all is an ordinary probe, not an error.
            return if lines.is_empty() && head.is_empty() {
                ReadOutcome::Disconnected
            } else {
                ReadOutcome::Rejected(HttpError::new(400, "connection closed mid-request"))
            };
        }
        head.extend_from_slice(&line);
        if head.len() > MAX_HTTP_HEAD_BYTES {
            return ReadOutcome::Rejected(HttpError::new(
                431,
                format!("request head exceeds {MAX_HTTP_HEAD_BYTES} bytes"),
            ));
        }
        if line.last() != Some(&b'\n') {
            return ReadOutcome::Rejected(HttpError::new(400, "truncated request head"));
        }
        let text = String::from_utf8_lossy(&line)
            .trim_end_matches(['\r', '\n'])
            .to_owned();
        if text.is_empty() {
            break;
        }
        if lines.len() > MAX_HTTP_HEADERS {
            return ReadOutcome::Rejected(HttpError::new(
                431,
                format!("more than {MAX_HTTP_HEADERS} header lines"),
            ));
        }
        lines.push(text);
    }

    let mut lines = lines.into_iter();
    let Some(request_line) = lines.next() else {
        return ReadOutcome::Rejected(HttpError::new(400, "empty request line"));
    };
    let mut parts = request_line.split_whitespace();
    let (Some(method), Some(target), Some(version)) = (parts.next(), parts.next(), parts.next())
    else {
        return ReadOutcome::Rejected(HttpError::new(400, "malformed request line"));
    };
    if !version.starts_with("HTTP/1.") {
        return ReadOutcome::Rejected(HttpError::new(
            505,
            format!("unsupported HTTP version `{version}`"),
        ));
    }

    let mut headers: Vec<(String, String)> = Vec::new();
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            return ReadOutcome::Rejected(HttpError::new(400, "malformed header line"));
        };
        headers.push((name.trim().to_ascii_lowercase(), value.trim().to_owned()));
    }
    if headers.len() > MAX_HTTP_HEADERS {
        return ReadOutcome::Rejected(HttpError::new(
            431,
            format!("more than {MAX_HTTP_HEADERS} header lines"),
        ));
    }

    let lookup = |name: &str| {
        headers
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    };
    if let Some(encoding) = lookup("transfer-encoding") {
        return ReadOutcome::Rejected(HttpError::new(
            501,
            format!("`Transfer-Encoding: {encoding}` is not implemented — send Content-Length"),
        ));
    }
    let length = match lookup("content-length") {
        None => 0usize,
        Some(raw) => match raw.trim().parse::<usize>() {
            Ok(length) => length,
            Err(_) => {
                return ReadOutcome::Rejected(HttpError::new(400, "malformed Content-Length"));
            }
        },
    };
    // Enforced before reading a single body byte: the shared ceiling is a
    // promise about allocation, not just about what is accepted.
    if length > MAX_REQUEST_BYTES {
        return ReadOutcome::Rejected(HttpError::new(
            413,
            format!("request body exceeds {MAX_REQUEST_BYTES} bytes"),
        ));
    }

    let mut body = vec![0u8; length];
    if length > 0 {
        let mut filled = 0usize;
        while filled < length {
            if Instant::now() >= deadline {
                return ReadOutcome::Rejected(HttpError::new(
                    408,
                    "timed out reading the request body",
                ));
            }
            match reader.read(&mut body[filled..]) {
                Ok(0) => {
                    return ReadOutcome::Rejected(HttpError::new(400, "truncated request body"));
                }
                Ok(read) => filled += read,
                Err(error) if error.kind() == ErrorKind::Interrupted => {}
                Err(error) if is_timeout(&error) => {
                    return ReadOutcome::Rejected(HttpError::new(
                        408,
                        "timed out reading the request body",
                    ));
                }
                Err(_) => return ReadOutcome::Disconnected,
            }
        }
    }

    let path = target.split(['?', '#']).next().unwrap_or("/").to_owned();
    ReadOutcome::Request(Box::new(HttpRequest {
        method: method.to_ascii_uppercase(),
        path,
        headers,
        body,
    }))
}

/// True for the error kinds a socket read/write timeout produces.
///
/// Platforms disagree: an expired `SO_RCVTIMEO` surfaces as `WouldBlock` on Unix
/// and `TimedOut` on Windows, so both are treated the same everywhere.
pub fn is_timeout(error: &std::io::Error) -> bool {
    matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut)
}

/// Serialize one response.
#[must_use]
pub fn response_bytes(status: u16, extra_headers: &[(&str, String)], body: &[u8]) -> Vec<u8> {
    let mut out = format!("HTTP/1.1 {status} {}\r\n", reason(status)).into_bytes();
    out.extend_from_slice(b"Content-Type: application/json\r\n");
    out.extend_from_slice(format!("Content-Length: {}\r\n", body.len()).as_bytes());
    // No cache, no sniffing, no cross-origin sharing: this transport is for a
    // local MCP client, never for a web page, and must not advertise otherwise.
    out.extend_from_slice(b"Cache-Control: no-store\r\n");
    out.extend_from_slice(b"X-Content-Type-Options: nosniff\r\n");
    out.extend_from_slice(b"Connection: close\r\n");
    for (name, value) in extra_headers {
        out.extend_from_slice(format!("{name}: {value}\r\n").as_bytes());
    }
    out.extend_from_slice(b"\r\n");
    out.extend_from_slice(body);
    out
}

/// Write every byte, retrying blocked writes until `timeout` elapses.
///
/// `Write::write_all` will not do: it retries only `Interrupted` and propagates
/// `WouldBlock` immediately, so on a socket with a send timeout it fails the
/// instant the peer's window fills — safe, but it would make
/// [`crate::mcp_host::limits::HTTP_WRITE_TIMEOUT`] decorative. An unbounded
/// retry loop is the opposite failure: a consumer that never reads would pin
/// the worker and its concurrency permit forever. So: retry, with a deadline.
pub fn write_all_deadline(
    writer: &mut impl Write,
    bytes: &[u8],
    timeout: Duration,
) -> std::io::Result<()> {
    let deadline = Instant::now() + timeout;
    let mut written = 0usize;
    while written < bytes.len() {
        if Instant::now() >= deadline {
            return Err(std::io::Error::new(
                ErrorKind::TimedOut,
                "peer did not drain the response before the write deadline",
            ));
        }
        match writer.write(&bytes[written..]) {
            Ok(0) => {
                return Err(std::io::Error::new(
                    ErrorKind::WriteZero,
                    "peer accepted no further bytes",
                ));
            }
            Ok(count) => written += count,
            Err(error) if error.kind() == ErrorKind::Interrupted => {}
            Err(error) if is_timeout(&error) => {
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(error) => return Err(error),
        }
    }
    match writer.flush() {
        Err(error) if is_timeout(&error) => Ok(()),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A peer that accepts the first `accepts` bytes and then never drains.
    ///
    /// A real socket would need its send buffer filled to reach this state,
    /// which is neither portable nor deterministic. The behaviour under test is
    /// the retry loop's deadline, not the kernel's buffering.
    struct StalledWriter {
        accepts: usize,
    }

    impl Write for StalledWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            if self.accepts == 0 {
                return Err(std::io::Error::new(ErrorKind::WouldBlock, "buffer full"));
            }
            let taken = self.accepts.min(buf.len());
            self.accepts -= taken;
            Ok(taken)
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn a_writer_that_never_drains_is_abandoned_at_the_deadline() {
        let mut writer = StalledWriter { accepts: 4 };
        let timeout = Duration::from_millis(200);
        let started = Instant::now();
        let error = write_all_deadline(&mut writer, b"0123456789", timeout)
            .expect_err("a peer that stops draining must not be waited on forever");
        let elapsed = started.elapsed();

        assert_eq!(error.kind(), ErrorKind::TimedOut);
        assert!(
            elapsed >= timeout,
            "gave up early after {elapsed:?}, before the {timeout:?} deadline"
        );
        assert!(
            elapsed < timeout * 10,
            "took {elapsed:?} — the deadline is not bounding the retry loop"
        );
    }

    #[test]
    fn a_draining_peer_receives_every_byte() {
        let mut sink = Vec::new();
        write_all_deadline(&mut sink, b"hello", Duration::from_secs(1))
            .expect("a sink never fills");
        assert_eq!(sink, b"hello");
    }

    #[test]
    fn every_response_closes_the_connection_and_refuses_sniffing() {
        let bytes = response_bytes(200, &[("Mcp-Session-Id", "abc".to_owned())], b"{}");
        let text = String::from_utf8(bytes).expect("responses are ASCII headers plus a JSON body");
        assert!(text.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(text.contains("Content-Length: 2\r\n"));
        assert!(text.contains("Connection: close\r\n"));
        assert!(text.contains("X-Content-Type-Options: nosniff\r\n"));
        assert!(text.contains("Cache-Control: no-store\r\n"));
        assert!(text.contains("Mcp-Session-Id: abc\r\n"));
        // No CORS: a web page must never be told it may read this.
        assert!(!text.to_ascii_lowercase().contains("access-control-allow"));
        assert!(text.ends_with("\r\n\r\n{}"));
    }
}
