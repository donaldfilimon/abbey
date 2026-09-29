//! Executor stream decoding for the chat TUI. Each backend's native stream
//! (NDJSON for claude / cursor-agent / grok, plain text for ollama / fm / abi)
//! becomes ordered [`StreamEvent`]s. Decoders never panic on bad input.

mod event;
pub(crate) mod plain;

pub use event::StreamEvent;

/// Incremental decoder fed raw stdout chunks in arrival order.
pub trait StreamDecoder: Send {
    fn feed(&mut self, chunk: &[u8]) -> Vec<StreamEvent>;
    fn finish(&mut self) -> Vec<StreamEvent>;
    /// Well-formed events of a type this decoder does not model.
    fn unknown_events(&self) -> u64 {
        0
    }
}
