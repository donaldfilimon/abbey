//! Source-bound schema3 fixture projection; expected JSON remains independently authored.
use abbey::app_core::{
    APP_SCHEMA_V4, APP_SCHEMA_V4_CONVERSATIONS, MAX_V4_CONTROL_BYTES, V4Command, V4Event,
};
use anyhow::{Context, Result};
use std::fmt::Write as _;
pub fn render(out: &mut String) -> Result<()> {
    let corpus: serde_json::Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/application-schema3/positive.json"
    ))
    .context("schema3 corpus")?;
    for (i, wire) in corpus["commands"]
        .as_array()
        .context("schema3 commands")?
        .iter()
        .enumerate()
    {
        let value: V4Command = serde_json::from_value(wire.clone()).context("schema3 command")?;
        super::emit(out, &format!("schema3Command{i}"), "V4Command", &value)?;
    }
    for (i, wire) in corpus["events"]
        .as_array()
        .context("schema3 events")?
        .iter()
        .enumerate()
    {
        let value: V4Event = serde_json::from_value(wire.clone()).context("schema3 event")?;
        super::emit(out, &format!("schema3Event{i}"), "V4Event", &value)?;
    }
    let legacy: serde_json::Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/application-schema3/schema2-conversation.json"
    ))?;
    let request: abbey::app_core::V4RequestEnvelope =
        serde_json::from_value(legacy["request"].clone())?;
    let response: abbey::app_core::V4ResponseEnvelope =
        serde_json::from_value(legacy["response"].clone())?;
    super::emit(out, "schema2EnvelopeRequest", "V4RequestEnvelope", &request)?;
    super::emit(
        out,
        "schema2EnvelopeResponse",
        "V4ResponseEnvelope",
        &response,
    )?;
    let request = abbey::app_core::V4RequestEnvelope {
        schema_version: APP_SCHEMA_V4,
        command: serde_json::from_value(corpus["commands"][0].clone())?,
        ..request
    };
    let event = serde_json::from_value(corpus["events"][0].clone())?;
    let response = abbey::app_core::V4ResponseEnvelope {
        schema_version: APP_SCHEMA_V4,
        payload: abbey::app_core::V4ResponsePayload::Ok { event },
        ..response
    };
    super::emit(out, "schema3EnvelopeRequest", "V4RequestEnvelope", &request)?;
    super::emit(
        out,
        "schema3EnvelopeResponse",
        "V4ResponseEnvelope",
        &response,
    )?;
    writeln!(
        out,
        "\n/** Schema3 requires ABS3 framing; compatibility selection is explicit. */\nexport const SUPPORTED_APPLICATION_SCHEMA = {APP_SCHEMA_V4} as const;\nexport const CONVERSATION_COMPATIBILITY_SCHEMA = {APP_SCHEMA_V4_CONVERSATIONS} as const;\nexport const DESKTOP_CONVERSATION_SCHEMA = {APP_SCHEMA_V4_CONVERSATIONS} as const;\nexport const MAX_CONTROL_PAYLOAD_BYTES = {MAX_V4_CONTROL_BYTES} as const;"
    )?;
    Ok(())
}
