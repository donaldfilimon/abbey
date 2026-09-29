//! Typed events decoded from an executor's live output stream.

/// One decoded unit of executor output, in arrival order.
#[derive(Debug, Clone, PartialEq)]
pub enum StreamEvent {
    TextDelta(String),
    ThinkingDelta(String),
    ToolStart {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    ToolEnd {
        id: String,
        ok: bool,
        output: String,
    },
    /// Only emitted when the executor itself reported token counts.
    Usage {
        input_tokens: u64,
        output_tokens: u64,
    },
    SessionId(String),
    Notice(String),
    /// Exit code of the executor; 130 means the user interrupted the turn.
    Done {
        exit: i32,
    },
    Failed(String),
}
