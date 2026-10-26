//! Schema3 envelope budgeting at typed whole-item boundaries only.
//! Stored content, fixed watermarks and original item identities are never rewritten.
use crate::app_core::{V4Event, V4ResponseEnvelope, V4ResponsePayload};

/// Remove one trailing item and publish a truthful continuation cursor.
/// A single oversized item refuses; text is never clipped or split implicitly.
pub(super) fn trim_tail(response: &mut V4ResponseEnvelope) -> bool {
    let V4ResponsePayload::Ok { event } = &mut response.payload else {
        return false;
    };
    if event.validate().is_err() {
        return false;
    }
    match event {
        V4Event::Conversations(page) if page.conversations.len() > 1 => {
            page.conversations.pop();
            page.next_after = page.conversations.last().map(|c| c.conversation_id.clone());
        }
        V4Event::Messages(page) if page.messages.len() > 1 => {
            page.messages.pop();
            page.next_after = page.messages.last().map(|m| m.sequence);
        }
        V4Event::Content(page) | V4Event::ContentEvents(page) if page.events.len() > 1 => {
            page.events.pop();
            page.next_after = page.events.last().map(|e| e.sequence);
        }
        _ => return false,
    }
    true
}

#[cfg(test)]
#[path = "v4_page_tests.rs"]
mod tests;
