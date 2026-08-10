//! Enforced, documented limits for the read-only MCP server (both transports).
//!
//! Every constant here is *enforced*, not advisory, and every one has a test
//! that drives the server past the boundary and asserts an error rather than a
//! panic, a hang, or an unbounded allocation.
//!
//! ## Transport-independent limits
//!
//! Applied identically to stdio frames and HTTP request bodies — the loopback
//! HTTP transport deliberately reuses these rather than defining parallel
//! ceilings. Tests live in the parent module's test module and
//! `tests/mcp_server.rs`.
//!
//! | constant | boundary test |
//! |---|---|
//! | [`MAX_REQUEST_BYTES`] | `tests::oversized_frames_are_rejected_and_the_session_survives`, `tests::a_frame_exactly_one_byte_past_the_ceiling_does_not_swallow_the_next_one`, and `http::tests::a_body_past_the_shared_request_ceiling_is_rejected_before_it_is_read` |
//! | [`MAX_RESPONSE_BYTES`] | `tests::a_response_past_the_size_ceiling_is_replaced_rather_than_written` (via a test-only handler — no registered tool can reach it); the HTTP body is built by the same `Server::encode_frame`, asserted by `http::tests::the_http_body_is_produced_by_the_shared_encode_frame` |
//! | [`MAX_JSON_DEPTH`] | `tests::nesting_past_the_depth_limit_is_rejected_before_any_tool_runs` |
//! | [`MAX_BATCH_SIZE`] | `tests::batches_are_rejected_at_the_documented_ceiling` |
//! | [`MAX_REQUEST_TIMEOUT`] | `tests::a_tool_that_outlives_its_deadline_times_out_instead_of_wedging_the_reader` |
//! | [`MAX_TOOL_NAME_BYTES`] | `tests::over_long_tool_names_and_protocol_versions_are_rejected` |
//! | [`MAX_PROTOCOL_VERSION_BYTES`] | `tests::over_long_tool_names_and_protocol_versions_are_rejected` |
//!
//! ## Loopback HTTP transport limits
//!
//! These exist only because a socket has failure modes a pipe does not: many
//! peers at once, a peer that never finishes, and a peer that repeats. Tests
//! live in [`super::http`]'s submodules and in `tests/mcp_http.rs`.
//!
//! | constant | boundary test |
//! |---|---|
//! | [`MAX_CONCURRENT_REQUESTS`] | `http::gate::tests::permits_are_capped_and_returned_on_drop` and `http::tests::connections_past_the_concurrency_cap_are_refused_and_the_server_recovers` |
//! | [`MAX_REQUESTS_PER_WINDOW`] | `http::gate::tests::the_rate_limiter_rejects_past_the_documented_threshold` and `http::tests::requests_past_the_rate_budget_are_answered_429` |
//! | [`RATE_LIMIT_WINDOW`] | `http::gate::tests::a_window_rollover_restores_the_rate_budget` |
//! | [`HTTP_READ_TIMEOUT`] | `http::tests::a_stalled_client_is_dropped_and_the_server_still_answers` |
//! | [`HTTP_WRITE_TIMEOUT`] | `http::wire::tests::a_writer_that_never_drains_is_abandoned_at_the_deadline` |
//! | [`MAX_HTTP_HEAD_BYTES`] | `http::tests::an_over_long_request_head_is_rejected` |
//! | [`MAX_HTTP_HEADERS`] | `http::tests::too_many_header_lines_are_rejected` |
//! | [`MAX_HTTP_SESSIONS`] | `http::session::tests::sessions_are_capped_and_idle_entries_are_reclaimed` |
//! | [`HTTP_SESSION_IDLE_TIMEOUT`] | `http::session::tests::sessions_are_capped_and_idle_entries_are_reclaimed` |

use std::time::Duration;

/// Largest single newline-delimited request frame accepted, in bytes.
///
/// The reader is *bounded* by this value (`Read::take`), so an adversarial
/// peer that never sends a newline cannot make the server allocate without
/// limit. A frame that exceeds it is rejected and the remainder of the line is
/// drained so the next frame still parses.
pub const MAX_REQUEST_BYTES: usize = 256 * 1024;

/// Largest response frame the server will emit, in bytes.
///
/// A response over this size is replaced by an internal error instead of being
/// written, so a tool cannot be used to push an unbounded stream at the client.
pub const MAX_RESPONSE_BYTES: usize = 1024 * 1024;

/// Maximum nesting depth accepted anywhere in a request's JSON structure.
///
/// This bounds both the parsed request document and, transitively, the argument
/// object handed to a tool. Deeply nested input is rejected before any tool
/// sees it, so recursive validation cannot blow the stack.
pub const MAX_JSON_DEPTH: usize = 16;

/// Maximum number of JSON-RPC messages accepted in one frame.
///
/// MCP removed JSON-RPC batching in its 2025-06-18 revision, so this server
/// accepts exactly one message per frame. The constant records the enforced
/// ceiling: a top-level array is rejected with `invalid request`, and raising
/// this number would be a deliberate protocol change, not a config tweak.
pub const MAX_BATCH_SIZE: usize = 1;

/// Wall-clock ceiling for a single `tools/call`.
///
/// The call runs on a worker thread and the reader waits on a channel; when the
/// deadline passes the client receives a timeout error and the server stays
/// responsive. Every tool registered today is fast and synchronous, so this is
/// a containment bound rather than a routinely-exercised path.
pub const MAX_REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

/// Maximum accepted length of a tool name, in bytes.
pub const MAX_TOOL_NAME_BYTES: usize = 64;

/// Maximum accepted length of a client-supplied protocol version string.
pub const MAX_PROTOCOL_VERSION_BYTES: usize = 32;

/// Maximum number of HTTP requests being serviced at once.
///
/// The transport serves exactly one request per connection, so "in-flight
/// request" and "connection being serviced" are the same thing here — the
/// permit is taken at `accept` and held until the socket is closed. A peer past
/// the cap is answered `503` inline on the accept thread and disconnected
/// **without spawning a worker**, so the thread count is bounded by this value
/// no matter how fast a local process dials in.
pub const MAX_CONCURRENT_REQUESTS: usize = 8;

/// Requests accepted per [`RATE_LIMIT_WINDOW`] before the server answers `429`.
///
/// This is a whole-listener budget, not per-peer: every client is on loopback,
/// so a source address carries no useful identity here and per-address buckets
/// would only invite spoofing questions that a loopback socket cannot answer.
pub const MAX_REQUESTS_PER_WINDOW: u32 = 120;

/// Fixed window over which [`MAX_REQUESTS_PER_WINDOW`] is counted.
pub const RATE_LIMIT_WINDOW: Duration = Duration::from_secs(60);

/// Wall-clock ceiling for reading one complete HTTP request from a peer.
///
/// Enforced as a socket read timeout *and* as an overall deadline across the
/// head and body, so a peer that trickles one byte at a time cannot hold its
/// concurrency permit open indefinitely by resetting the per-read timer.
pub const HTTP_READ_TIMEOUT: Duration = Duration::from_secs(10);

/// Wall-clock ceiling for writing one complete HTTP response to a peer.
///
/// A consumer that stops reading eventually fills the socket buffer and the
/// write starts returning `WouldBlock`/`TimedOut`. The writer retries until this
/// deadline and then abandons the connection — `write_all` alone would not do:
/// it propagates `WouldBlock` immediately, which is safe but proves nothing, and
/// an unbounded retry loop would pin the worker forever.
pub const HTTP_WRITE_TIMEOUT: Duration = Duration::from_secs(10);

/// Largest accepted HTTP request line plus header block, in bytes.
///
/// Bounded independently of [`MAX_REQUEST_BYTES`] because the head is read
/// before any `Content-Length` is known — this is what stops a peer that sends
/// headers forever.
pub const MAX_HTTP_HEAD_BYTES: usize = 8 * 1024;

/// Largest accepted number of HTTP header lines in one request.
pub const MAX_HTTP_HEADERS: usize = 64;

/// Maximum number of live `Mcp-Session-Id` sessions retained at once.
///
/// Each session owns one [`super::Server`], so this bounds the memory a peer can
/// pin by calling `initialize` repeatedly. Past the cap the server sweeps idle
/// sessions first and only then refuses with `503`.
pub const MAX_HTTP_SESSIONS: usize = 16;

/// How long a session survives without a request before it is reclaimed.
pub const HTTP_SESSION_IDLE_TIMEOUT: Duration = Duration::from_secs(300);

/// Default TCP port for `abbey mcp serve http` when `--port` is omitted.
///
/// Not a security limit; recorded here so the one place that documents the
/// transport's numbers documents all of them. `--port 0` asks the OS for an
/// ephemeral port, which is what the tests use.
pub const DEFAULT_HTTP_PORT: u16 = 8787;
