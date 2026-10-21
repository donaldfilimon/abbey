//! Visible transcript events only: no raw tool inputs, credentials, or provider reasoning.
use super::v4::{ensure, label, limit, timestamp};
use super::*;
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Clone, PartialEq, Eq, Serialize)]
#[serde(
    tag = "kind",
    content = "data",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum V4ContentPayload {
    TextDelta {
        text: String,
    },
    Tool {
        tool_id: String,
        state: V4ToolState,
    },
    Notice {
        code: V4NoticeCode,
    },
    Usage {
        input_tokens: u32,
        output_tokens: u32,
    },
    Terminal {
        state: RunState,
    },
}
impl V4ContentPayload {
    pub fn validate(&self) -> Result<(), ValidationError> {
        match self {
            Self::TextDelta { text: value } => ensure(
                !value.is_empty() && value.len() <= MAX_V4_DELTA_BYTES && !value.contains('\0'),
            ),
            Self::Tool { tool_id, .. } => label(tool_id),
            Self::Terminal { state } => ensure(state.is_terminal()),
            _ => Ok(()),
        }
    }
}
impl fmt::Debug for V4ContentPayload {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("V4ContentPayload([redacted])")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum V4ToolState {
    Started,
    Succeeded,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum V4NoticeCode {
    Truncated,
    Reconnecting,
    ProviderUnavailable,
    RecoveryRequired,
}

#[derive(Clone, PartialEq, Eq, Serialize)]
pub struct V4ContentEvent {
    pub conversation_id: ConversationId,
    pub run_id: RunId,
    pub sequence: u64,
    pub timestamp_ms: u64,
    pub payload: V4ContentPayload,
}

impl V4ContentEvent {
    pub fn validate(&self) -> Result<(), ValidationError> {
        timestamp(self.timestamp_ms)?;
        ensure(self.sequence > 0 && self.sequence <= MAX_V4_CONTENT_EVENTS)?;
        self.payload.validate()
    }
}
impl fmt::Debug for V4ContentEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("V4ContentEvent([redacted])")
    }
}
impl V4ContentEvent {
    /// Require exact correlation and a strictly increasing sequence; filtered per-run pages
    /// may contain gaps because the durable sequence is conversation-scoped.
    pub fn validate_after(
        &self,
        conversation: &ConversationId,
        run: &RunId,
        after: u64,
    ) -> Result<(), ValidationError> {
        self.validate()?;
        ensure(
            &self.conversation_id == conversation && &self.run_id == run && self.sequence > after,
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct V4ContentQuery {
    pub conversation_id: ConversationId,
    pub run_id: RunId,
    pub after: u64,
    pub limit: u16,
}

impl V4ContentQuery {
    pub fn validate(&self) -> Result<(), ValidationError> {
        limit(self.limit)?;
        ensure(self.after <= MAX_V4_CONTENT_EVENTS)
    }
}

#[derive(Clone, PartialEq, Eq, Serialize)]
pub struct V4ContentPage {
    pub conversation_id: ConversationId,
    pub run_id: RunId,
    pub after: u64,
    pub events: Vec<V4ContentEvent>,
    pub next_after: Option<u64>,
}

impl V4ContentPage {
    pub fn validate(&self) -> Result<(), ValidationError> {
        super::v4::page_bytes(self)?;
        ensure(
            self.after <= MAX_V4_CONTENT_EVENTS && self.events.len() <= usize::from(MAX_V4_PAGE),
        )?;
        let mut last = self.after;
        let mut terminal = false;
        let mut bytes = 0;
        for e in &self.events {
            ensure(!terminal)?;
            e.validate_after(&self.conversation_id, &self.run_id, last)?;
            last = e.sequence;
            terminal = matches!(e.payload, V4ContentPayload::Terminal { .. });
            if let V4ContentPayload::TextDelta { text } = &e.payload {
                bytes += text.len();
            }
        }
        ensure(
            bytes <= MAX_V4_CONTENT_BYTES
                && self
                    .next_after
                    .is_none_or(|s| !terminal && !self.events.is_empty() && s == last),
        )
    }
}
impl fmt::Debug for V4ContentPage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("V4ContentPage([redacted])")
    }
}

impl<'de> Deserialize<'de> for V4ContentEvent {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            conversation_id: ConversationId,
            run_id: RunId,
            sequence: u64,
            timestamp_ms: u64,
            payload: V4ContentPayload,
        }
        let w = Wire::deserialize(deserializer)
            .map_err(|_| serde::de::Error::custom("invalid v4 contract"))?;
        let value = Self {
            conversation_id: w.conversation_id,
            run_id: w.run_id,
            sequence: w.sequence,
            timestamp_ms: w.timestamp_ms,
            payload: w.payload,
        };
        value.validate().map_err(serde::de::Error::custom)?;
        Ok(value)
    }
}

impl<'de> Deserialize<'de> for V4ContentQuery {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            conversation_id: ConversationId,
            run_id: RunId,
            after: u64,
            limit: u16,
        }
        let w = Wire::deserialize(deserializer)
            .map_err(|_| serde::de::Error::custom("invalid v4 contract"))?;
        let value = Self {
            conversation_id: w.conversation_id,
            run_id: w.run_id,
            after: w.after,
            limit: w.limit,
        };
        value.validate().map_err(serde::de::Error::custom)?;
        Ok(value)
    }
}

impl<'de> Deserialize<'de> for V4ContentPage {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            conversation_id: ConversationId,
            run_id: RunId,
            after: u64,
            events: Vec<V4ContentEvent>,
            next_after: Option<u64>,
        }
        let w = Wire::deserialize(deserializer)
            .map_err(|_| serde::de::Error::custom("invalid v4 contract"))?;
        let value = Self {
            conversation_id: w.conversation_id,
            run_id: w.run_id,
            after: w.after,
            events: w.events,
            next_after: w.next_after,
        };
        value.validate().map_err(serde::de::Error::custom)?;
        Ok(value)
    }
}

impl<'de> Deserialize<'de> for V4ContentPayload {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(
            tag = "kind",
            content = "data",
            rename_all = "snake_case",
            deny_unknown_fields
        )]
        enum Wire {
            TextDelta {
                text: String,
            },
            Tool {
                tool_id: String,
                state: V4ToolState,
            },
            Notice {
                code: V4NoticeCode,
            },
            Usage {
                input_tokens: u32,
                output_tokens: u32,
            },
            Terminal {
                state: RunState,
            },
        }
        let value = match Wire::deserialize(deserializer)
            .map_err(|_| serde::de::Error::custom("invalid v4 contract"))?
        {
            Wire::TextDelta { text } => Self::TextDelta { text },
            Wire::Tool { tool_id, state } => Self::Tool { tool_id, state },
            Wire::Notice { code } => Self::Notice { code },
            Wire::Usage {
                input_tokens,
                output_tokens,
            } => Self::Usage {
                input_tokens,
                output_tokens,
            },
            Wire::Terminal { state } => Self::Terminal { state },
        };
        value.validate().map_err(serde::de::Error::custom)?;
        Ok(value)
    }
}

impl<'de> Deserialize<'de> for V4ToolState {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "snake_case")]
        enum Wire {
            Started,
            Succeeded,
            Failed,
            Cancelled,
        }
        Ok(
            match Wire::deserialize(deserializer)
                .map_err(|_| serde::de::Error::custom("invalid v4 contract"))?
            {
                Wire::Started => Self::Started,
                Wire::Succeeded => Self::Succeeded,
                Wire::Failed => Self::Failed,
                Wire::Cancelled => Self::Cancelled,
            },
        )
    }
}

impl<'de> Deserialize<'de> for V4NoticeCode {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "snake_case")]
        enum Wire {
            Truncated,
            Reconnecting,
            ProviderUnavailable,
            RecoveryRequired,
        }
        Ok(
            match Wire::deserialize(deserializer)
                .map_err(|_| serde::de::Error::custom("invalid v4 contract"))?
            {
                Wire::Truncated => Self::Truncated,
                Wire::Reconnecting => Self::Reconnecting,
                Wire::ProviderUnavailable => Self::ProviderUnavailable,
                Wire::RecoveryRequired => Self::RecoveryRequired,
            },
        )
    }
}
