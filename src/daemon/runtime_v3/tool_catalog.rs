//! Startup-bound protocol-v3 tool catalog for the authenticated daemon.

use abi_agent_runtime::{ToolEffect, ToolSpec};
use jsonschema::Validator;
use serde_json::json;

use crate::app_core::{V3ToolDescriptor, V3ToolEffect};
use crate::edition::{ACTIVE, Edition};

pub(super) const MEMORY_MARK_OBSOLETE_TOOL_ID: &str = "abbey_memory_mark_obsolete";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ToolRoute {
    DirectReadOnly,
    ApprovalRequired,
}

pub(super) struct BoundTool {
    pub(super) descriptor: V3ToolDescriptor,
    pub(super) spec: ToolSpec,
    pub(super) validator: Validator,
    pub(super) route: ToolRoute,
}

pub(super) fn build() -> Result<Vec<BoundTool>, ()> {
    let descriptors = crate::mcp_host::v3_descriptors().map_err(|_| ())?;
    let specs = crate::mcp_host::v3_specs().map_err(|_| ())?;
    if descriptors.len() != specs.len() {
        return Err(());
    }
    let mut tools = descriptors
        .into_iter()
        .zip(specs)
        .map(|(descriptor, spec)| bind(descriptor, spec, ToolRoute::DirectReadOnly))
        .collect::<Result<Vec<_>, ()>>()?;

    if ACTIVE == Edition::Safe {
        let input_schema = json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "type": "object",
            "properties": {
                "record_id": {
                    "type": "string",
                    "minLength": 1,
                    "maxLength": 128,
                    "pattern": "^[A-Za-z0-9._:-]+$"
                }
            },
            "required": ["record_id"],
            "additionalProperties": false
        });
        let description = "Mark one Abbey memory record obsolete without deleting its provenance. The exact call requires a durable safe-edition approval and an identical explicit resubmission before execution.";
        let descriptor = V3ToolDescriptor {
            tool_id: MEMORY_MARK_OBSOLETE_TOOL_ID.to_owned(),
            description: description.to_owned(),
            effect: V3ToolEffect::Mutating,
            input_schema: input_schema.clone(),
        };
        descriptor.validate().map_err(|_| ())?;
        let input_schema = serde_json::to_string(&input_schema).map_err(|_| ())?;
        let spec = ToolSpec::new(MEMORY_MARK_OBSOLETE_TOOL_ID)
            .with_description(description)
            .with_input_schema(input_schema)
            .with_effect(ToolEffect::Mutating);
        tools.push(bind(descriptor, spec, ToolRoute::ApprovalRequired)?);
    }
    Ok(tools)
}

fn bind(descriptor: V3ToolDescriptor, spec: ToolSpec, route: ToolRoute) -> Result<BoundTool, ()> {
    if descriptor.tool_id != spec.name {
        return Err(());
    }
    let effect_matches_route = matches!(
        (route, descriptor.effect, spec.effect),
        (
            ToolRoute::DirectReadOnly,
            V3ToolEffect::ReadOnly,
            ToolEffect::ReadOnly
        ) | (
            ToolRoute::ApprovalRequired,
            V3ToolEffect::Mutating,
            ToolEffect::Mutating
        )
    );
    if !effect_matches_route {
        return Err(());
    }
    let validator = jsonschema::validator_for(&descriptor.input_schema).map_err(|_| ())?;
    Ok(BoundTool {
        descriptor,
        spec,
        validator,
        route,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn daemon_mutation_is_safe_edition_only_and_never_enters_mcp() {
        let tools = build().unwrap();
        let expected = crate::mcp_host::SAFE_TOOLS.len()
            + usize::from(crate::edition::ACTIVE == crate::edition::Edition::Safe);
        assert_eq!(tools.len(), expected);
        assert_eq!(
            crate::mcp_host::tool_names(),
            vec!["abbey_status", "abbey_claims", "abbey_platform"]
        );
        assert!(
            crate::mcp_host::tool_names()
                .iter()
                .all(|name| *name != MEMORY_MARK_OBSOLETE_TOOL_ID)
        );
        if crate::edition::ACTIVE == crate::edition::Edition::Personal {
            assert!(
                tools
                    .iter()
                    .all(|tool| tool.descriptor.tool_id != MEMORY_MARK_OBSOLETE_TOOL_ID)
            );
        }
    }
}
