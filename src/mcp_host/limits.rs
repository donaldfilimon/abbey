//! Enforced, documented limits for the read-only stdio MCP server.
//!
//! Every constant here is *enforced*, not advisory, and every one has a test in
//! [`super::tests`] (several also in `tests/mcp_server.rs`) that drives the
//! server past the boundary and asserts a JSON-RPC error rather than a panic, a
//! hang, or an unbounded allocation:
//!
//! | constant | boundary test |
//! |---|---|
//! | [`MAX_REQUEST_BYTES`] | `oversized_frames_are_rejected_and_the_session_survives` plus `a_frame_exactly_one_byte_past_the_ceiling_does_not_swallow_the_next_one` |
//! | [`MAX_RESPONSE_BYTES`] | `a_response_past_the_size_ceiling_is_replaced_rather_than_written` (via a test-only handler — no registered tool can reach it) |
//! | [`MAX_JSON_DEPTH`] | `nesting_past_the_depth_limit_is_rejected_before_any_tool_runs` |
//! | [`MAX_BATCH_SIZE`] | `batches_are_rejected_at_the_documented_ceiling` |
//! | [`MAX_REQUEST_TIMEOUT`] | `a_tool_that_outlives_its_deadline_times_out_instead_of_wedging_the_reader` |
//! | [`MAX_TOOL_NAME_BYTES`] | `over_long_tool_names_and_protocol_versions_are_rejected` |
//! | [`MAX_PROTOCOL_VERSION_BYTES`] | `over_long_tool_names_and_protocol_versions_are_rejected` |

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
