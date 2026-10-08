//! Executor stream decoding for the chat TUI. Each backend's native stream
//! (NDJSON for claude / cursor-agent / grok, plain text for ollama / fm / abi)
//! becomes ordered [`StreamEvent`]s. Decoders never panic on bad input.

pub(crate) mod claude;
pub(crate) mod cursor;
mod event;
pub(crate) mod grok;
pub(crate) mod ndjson;
pub(crate) mod plain;
mod tap;

pub use event::StreamEvent;
pub use tap::StreamTap;

use crate::agent::AgentBackend;

/// Incremental decoder fed raw stdout chunks in arrival order.
pub trait StreamDecoder: Send {
    fn feed(&mut self, chunk: &[u8]) -> Vec<StreamEvent>;
    fn finish(&mut self) -> Vec<StreamEvent>;
    /// Well-formed events of a type this decoder does not model.
    fn unknown_events(&self) -> u64 {
        0
    }
}

pub(crate) fn decoder_for(backend: AgentBackend) -> Box<dyn StreamDecoder> {
    match backend {
        AgentBackend::Claude => Box::new(claude::ClaudeDecoder::default()),
        AgentBackend::Cursor => Box::new(cursor::CursorDecoder::default()),
        AgentBackend::Grok => Box::new(grok::GrokDecoder::default()),
        AgentBackend::Fm | AgentBackend::Ollama | AgentBackend::Abi => {
            Box::new(plain::PlainDecoder::default())
        }
    }
}

pub(crate) fn stream_output_format(backend: AgentBackend) -> Option<&'static str> {
    match backend {
        AgentBackend::Claude | AgentBackend::Cursor => Some("stream-json"),
        AgentBackend::Grok => Some("streaming-json"),
        AgentBackend::Fm | AgentBackend::Ollama | AgentBackend::Abi => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::AgentBackend;

    #[test]
    fn every_backend_has_a_decoder_and_only_structured_ones_have_a_format() {
        for b in [
            AgentBackend::Cursor,
            AgentBackend::Grok,
            AgentBackend::Fm,
            AgentBackend::Abi,
            AgentBackend::Claude,
            AgentBackend::Ollama,
        ] {
            let mut d = decoder_for(b);
            let _ = d.feed(b"");
        }
        assert_eq!(
            stream_output_format(AgentBackend::Claude),
            Some("stream-json")
        );
        assert_eq!(
            stream_output_format(AgentBackend::Cursor),
            Some("stream-json")
        );
        assert_eq!(
            stream_output_format(AgentBackend::Grok),
            Some("streaming-json")
        );
        assert_eq!(stream_output_format(AgentBackend::Ollama), None);
        assert_eq!(stream_output_format(AgentBackend::Fm), None);
        assert_eq!(stream_output_format(AgentBackend::Abi), None);
    }
}
