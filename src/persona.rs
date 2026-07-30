//! Abbey/Aviva/Abi persona routing via `abi-ai` contracts.

use abi_ai::{
    AgentProfile, ProfileWeights, explicit_profile_selector, profile_contract, route_profile,
};

/// Parse a persona name or return None.
pub fn parse_persona(s: &str) -> Option<AgentProfile> {
    match s.trim().to_ascii_lowercase().as_str() {
        "abbey" => Some(AgentProfile::Abbey),
        "aviva" => Some(AgentProfile::Aviva),
        "abi" => Some(AgentProfile::Abi),
        _ => None,
    }
}

/// Select persona: env `ABBEY_PERSONA` if set to a known name, else explicit
/// address in input, else keyword router.
pub fn select_persona(input: &str) -> AgentProfile {
    if let Ok(v) = std::env::var("ABBEY_PERSONA") {
        if let Some(p) = parse_persona(&v) {
            return p;
        }
    }
    if let Some(p) = explicit_profile_selector(input) {
        return p;
    }
    route_profile(input)
}

/// Wrap a user prompt with the frozen persona contract prefix/suffix.
pub fn wrap_prompt(profile: AgentProfile, user: &str) -> String {
    let c = profile_contract(profile);
    format!(
        "{}{}{}",
        c.response_prefix,
        user.trim_end(),
        c.response_suffix
    )
}

/// Doctor / TUI status lines.
pub fn persona_status_lines(input_hint: &str) -> Vec<String> {
    let selected = select_persona(input_hint);
    let prior = ProfileWeights::prior();
    vec![
        format!(
            "persona:     {} (from router/env/explicit)",
            selected.label()
        ),
        format!(
            "prior weights abbey={:.2} aviva={:.2} abi={:.2}",
            prior.w_abbey, prior.w_aviva, prior.w_abi
        ),
        "personas:    Abbey (default) · Aviva (direct) · Abi (orchestrator)".into(),
        "source:      abi-ai identity + router (frozen contracts)".into(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_names() {
        assert_eq!(parse_persona("Aviva"), Some(AgentProfile::Aviva));
        assert_eq!(parse_persona("ABI"), Some(AgentProfile::Abi));
        assert!(parse_persona("nope").is_none());
    }

    #[test]
    fn explicit_aviva_wins() {
        assert_eq!(
            select_persona("Aviva, summarize this file"),
            AgentProfile::Aviva
        );
    }

    #[test]
    fn wrap_contains_prefix() {
        let out = wrap_prompt(AgentProfile::Abbey, "hello");
        assert!(out.starts_with("Abbey: "));
        assert!(out.contains("hello"));
    }
}
