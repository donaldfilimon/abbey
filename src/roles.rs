//! Max / Gemma worker roles — bindings to cursor-agent model ids, not local weights.

/// Rebindable worker role (technical vs visual/conversational).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkerRole {
    /// Technical executor (code, tools, math, automation).
    Max,
    /// Visual / conversational / tone-sensitive work.
    Gemma,
    /// Classify from the prompt.
    Auto,
}

impl WorkerRole {
    pub fn label(self) -> &'static str {
        match self {
            Self::Max => "max",
            Self::Gemma => "gemma",
            Self::Auto => "auto",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "max" | "qwen" | "technical" => Some(Self::Max),
            "gemma" | "visual" | "chat" => Some(Self::Gemma),
            "auto" | "default" => Some(Self::Auto),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskClass {
    Code,
    Tool,
    Math,
    Visual,
    Conversational,
    Hybrid,
    Other,
}

/// Keyword heuristic for task class.
pub fn classify_task(input: &str) -> TaskClass {
    let lower = input.to_ascii_lowercase();
    let code = [
        "code",
        "refactor",
        "compile",
        "rust",
        "patch",
        "test",
        "bug",
        "fix",
        "implement",
        "function",
        "cargo",
        "clippy",
        "api",
        "debug",
    ]
    .iter()
    .any(|k| lower.contains(k));
    let tool = ["tool", "shell", "command", "mcp", "deploy", "git ", "curl"]
        .iter()
        .any(|k| lower.contains(k));
    let math = [
        "math",
        "prove",
        "equation",
        "integral",
        "derive",
        "calculate",
    ]
    .iter()
    .any(|k| lower.contains(k));
    let visual = [
        "image",
        "screenshot",
        "photo",
        "visual",
        "ui mock",
        "diagram",
        "picture",
        "ocr",
    ]
    .iter()
    .any(|k| lower.contains(k));
    let conversational = [
        "feel",
        "tone",
        "explain gently",
        "conversation",
        "empath",
        "how are you",
        "story",
    ]
    .iter()
    .any(|k| lower.contains(k));

    let tech = code || tool || math;
    let soft = visual || conversational;
    if tech && soft {
        TaskClass::Hybrid
    } else if code {
        TaskClass::Code
    } else if tool {
        TaskClass::Tool
    } else if math {
        TaskClass::Math
    } else if visual {
        TaskClass::Visual
    } else if conversational {
        TaskClass::Conversational
    } else {
        TaskClass::Other
    }
}

/// Abi routing decision: primary role, confidence, and optional alternate.
#[derive(Debug, Clone, PartialEq)]
pub struct RouteDecision {
    pub primary: WorkerRole,
    pub confidence: f32,
    /// Secondary role worth considering (e.g. Gemma after Max on Hybrid).
    pub alternate: Option<WorkerRole>,
    /// Human-readable fallback note when confidence is low (no auto-reinvoke).
    pub fallback: Option<String>,
    pub class: TaskClass,
}

/// Classify and score a routing decision for the route log.
///
/// Does **not** re-invoke agents on low confidence — it only records what Abi
/// would fall back to, keeping the one canonical execution path.
pub fn route_decision(input: &str, override_role: Option<WorkerRole>) -> RouteDecision {
    let class = classify_task(input);

    if let Some(r) = override_role.filter(|r| *r != WorkerRole::Auto) {
        return RouteDecision {
            primary: r,
            confidence: 0.95,
            alternate: None,
            fallback: None,
            class,
        };
    }
    if let Ok(v) = std::env::var("ABBEY_ROLE") {
        if let Some(r) = WorkerRole::parse(&v).filter(|r| *r != WorkerRole::Auto) {
            return RouteDecision {
                primary: r,
                confidence: 0.9,
                alternate: None,
                fallback: None,
                class,
            };
        }
    }

    match class {
        TaskClass::Code | TaskClass::Tool | TaskClass::Math => RouteDecision {
            primary: WorkerRole::Max,
            confidence: 0.85,
            alternate: None,
            fallback: None,
            class,
        },
        TaskClass::Visual | TaskClass::Conversational => RouteDecision {
            primary: WorkerRole::Gemma,
            confidence: 0.85,
            alternate: None,
            fallback: None,
            class,
        },
        // Abi coordinates: Max leads; Gemma is the recorded alternate for the
        // visual/conversational half (hybrid-loop uses both stages explicitly).
        TaskClass::Hybrid => RouteDecision {
            primary: WorkerRole::Max,
            confidence: 0.7,
            alternate: Some(WorkerRole::Gemma),
            fallback: Some("hybrid: prefer hybrid-loop or /gemma for visual half".into()),
            class,
        },
        TaskClass::Other => RouteDecision {
            primary: WorkerRole::Max,
            confidence: 0.55,
            alternate: Some(WorkerRole::Gemma),
            fallback: Some("low-confidence Other class; Max default, Gemma alternate".into()),
            class,
        },
    }
}

/// Default Abbey alias for a resolved role. These are **cursor-agent model
/// aliases**, not in-process Qwen/Gemma checkpoints.
pub fn default_model_for_role(role: WorkerRole) -> &'static str {
    match role {
        WorkerRole::Max | WorkerRole::Auto => "fable",
        WorkerRole::Gemma => "composer",
    }
}

pub fn role_system_note(role: WorkerRole) -> String {
    match role {
        WorkerRole::Max | WorkerRole::Auto => {
            "Worker role: Max (technical). Prefer complete, testable changes; \
             inspect before editing; report what was and was not verified."
                .into()
        }
        WorkerRole::Gemma => {
            "Worker role: Gemma (visual/conversational). Prioritize clear human-facing \
             interpretation, tone, and multimodal description when relevant."
                .into()
        }
    }
}

pub fn role_status_lines(max_model: &str, gemma_model: &str) -> Vec<String> {
    vec![
        format!("role.max →   {max_model} (cursor-agent binding)"),
        format!("role.gemma → {gemma_model} (cursor-agent binding)"),
        "note:        Max/Gemma are roles, not local model weights".into(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_selects_max() {
        assert_eq!(
            classify_task("refactor this rust function"),
            TaskClass::Code
        );
        assert_eq!(
            route_decision("fix the compile error", Some(WorkerRole::Auto)).primary,
            WorkerRole::Max
        );
    }

    #[test]
    fn image_selects_gemma() {
        assert_eq!(classify_task("describe this screenshot"), TaskClass::Visual);
        assert_eq!(
            route_decision("describe this screenshot", None).primary,
            WorkerRole::Gemma
        );
    }

    #[test]
    fn override_wins() {
        assert_eq!(
            route_decision("refactor code", Some(WorkerRole::Gemma)).primary,
            WorkerRole::Gemma
        );
    }

    #[test]
    fn hybrid_records_gemma_alternate_and_fallback() {
        let d = route_decision("refactor this UI screenshot layout in rust", None);
        assert_eq!(d.primary, WorkerRole::Max);
        assert_eq!(d.alternate, Some(WorkerRole::Gemma));
        assert!(d.fallback.is_some());
        assert!(d.confidence < 0.85);
        assert_eq!(d.class, TaskClass::Hybrid);
    }

    #[test]
    fn other_is_low_confidence_with_alternate() {
        let d = route_decision("hmm what about that", None);
        assert_eq!(d.class, TaskClass::Other);
        assert!(d.confidence < 0.7);
        assert_eq!(d.alternate, Some(WorkerRole::Gemma));
    }
}
