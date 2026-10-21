//! Bounded persistence of provider-visible content.
use super::*;

/// Persist visible v4 text deltas while enforcing the same boundary rules as
/// [`ProviderBoundarySink`]. Write failures invalidate the run.
pub(super) struct V4ContentSink<'a> {
    store: &'a RuntimeStore,
    run_id: &'a RunId,
    expected_model: &'a str,
    started: bool,
    terminal: bool,
    invalid: bool,
}

impl<'a> V4ContentSink<'a> {
    pub(super) const fn new(
        store: &'a RuntimeStore,
        run_id: &'a RunId,
        expected_model: &'a str,
    ) -> Self {
        Self {
            store,
            run_id,
            expected_model,
            started: false,
            terminal: false,
            invalid: false,
        }
    }

    pub(super) fn validate(&self) -> Result<(), ExecutionError> {
        if self.started && self.terminal && !self.invalid {
            Ok(())
        } else {
            Err(unsupported())
        }
    }

    pub(super) fn now_ms() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX))
            .unwrap_or(0)
    }
}

impl EventSink for V4ContentSink<'_> {
    fn emit(&mut self, event: &ModelEvent) {
        match event {
            ModelEvent::Started { model }
                if !self.started && !self.terminal && model == self.expected_model =>
            {
                self.started = true;
            }
            ModelEvent::TextDelta { text } if self.started && !self.terminal => {
                if text.is_empty() {
                    return;
                }
                let mut remaining = text.as_str();
                while !remaining.is_empty() {
                    let mut end = remaining.len().min(crate::app_core::MAX_V4_DELTA_BYTES);
                    while !remaining.is_char_boundary(end) {
                        end -= 1;
                    }
                    if self
                        .store
                        .v4_append_content(
                            self.run_id,
                            V4ContentPayload::TextDelta {
                                text: remaining[..end].to_owned(),
                            },
                            Self::now_ms(),
                        )
                        .is_err()
                    {
                        self.invalid = true;
                        break;
                    }
                    remaining = &remaining[end..];
                }
            }
            ModelEvent::Finished { .. } if self.started && !self.terminal => {
                self.terminal = true;
            }
            ModelEvent::ToolCall(_) | ModelEvent::ToolResult(_) => self.invalid = true,
            _ => self.invalid = true,
        }
    }
}
