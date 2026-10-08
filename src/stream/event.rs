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

impl StreamEvent {
    pub(crate) fn payload_bytes(&self) -> usize {
        match self {
            Self::TextDelta(s)
            | Self::ThinkingDelta(s)
            | Self::SessionId(s)
            | Self::Notice(s)
            | Self::Failed(s) => s.len(),
            Self::ToolStart { id, name, input } => {
                id.len() + name.len() + serde_json::to_vec(input).map_or(0, |v| v.len())
            }
            Self::ToolEnd { id, output, .. } => id.len() + output.len(),
            Self::Usage { .. } | Self::Done { .. } => 0,
        }
    }
}
