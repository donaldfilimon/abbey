//! Abbey's own machine-readable self-description as a training corpus.
//!
//! "Train on the specs of itself" means exactly this: the capability claim
//! registry (`src/claims/registry.rs`, compiled into the binary) and the goal
//! ledger (`tasks/goals.md`). Both are already machine-readable, both are
//! already the repository's answer to "what is Abbey and what is it doing",
//! and neither needs to be invented for a corpus.
//!
//! No other corpus is fabricated here. In particular there is no Quesar
//! corpus: Quesar has no code, crate, config, or edition anywhere in this
//! repository — it is a brand boundary (`docs/brand.md`) and nothing else, so
//! there is nothing of its own to train on.

use crate::claims::CLAIMS;
use crate::train::dataset::Pair;
use crate::train::redaction::redact;

/// One goal parsed out of `tasks/goals.md`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoalSpec {
    /// Stable goal id.
    pub id: String,
    /// The `## ` heading text.
    pub title: String,
    /// The declared status.
    pub status: String,
    /// The declared implementation evidence.
    pub implementation: String,
    /// The declared automated-test evidence.
    pub automated_test: String,
    /// The declared live/external evidence.
    pub live_external: String,
    /// The declared next action.
    pub next_action: String,
    /// The declared blocker owner, when the goal has one.
    pub blocker_owner: Option<String>,
}

/// What the specs corpus actually found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SpecsReport {
    /// Claim-registry pairs.
    pub claims: usize,
    /// Goal-ledger pairs.
    pub goals: usize,
    /// Where the goal ledger was read from, when it was found.
    pub goals_path: Option<String>,
    /// Goal blocks that were present but malformed and therefore skipped.
    pub skipped_goal_blocks: usize,
    /// How many pairs the redaction changed.
    pub redacted_pairs: usize,
}

/// Build the specs corpus from the compiled claim registry and a goal ledger.
#[must_use]
pub fn corpus(goals_path: Option<&std::path::Path>) -> (Vec<Pair>, SpecsReport) {
    let mut pairs = Vec::new();
    let mut report = SpecsReport::default();

    for claim in CLAIMS {
        let mut response = format!(
            "Capability {id} — {name}. Status: {status}. Evidence boundary: {note}.",
            id = claim.id,
            name = claim.name,
            status = claim.status.label(),
            note = claim.note
        );
        if let Some(instead) = claim.instead {
            response.push_str(&format!(" Use {instead} instead."));
        }
        let (pair, changed) = pair(
            &format!("specs:claim:{}", claim.id),
            &format!(
                "Abbey capability {}: what is it, what is its status, and what is its evidence boundary?",
                claim.id
            ),
            &response,
        );
        report.redacted_pairs += usize::from(changed);
        pairs.push(pair);
    }
    report.claims = pairs.len();

    if let Some(path) = goals_path
        && let Ok(text) = std::fs::read_to_string(path)
    {
        let (goals, skipped) = parse_goals(&text);
        report.skipped_goal_blocks = skipped;
        report.goals_path = Some(path.display().to_string());
        for goal in &goals {
            let mut response = format!(
                "Goal {id} — {title}. Status: {status}. Implementation evidence: {implementation}. \
                 Automated-test evidence: {automated_test}. Live/external evidence: {live_external}. \
                 Next action: {next_action}.",
                id = goal.id,
                title = goal.title,
                status = goal.status,
                implementation = goal.implementation,
                automated_test = goal.automated_test,
                live_external = goal.live_external,
                next_action = goal.next_action
            );
            if let Some(owner) = &goal.blocker_owner {
                response.push_str(&format!(" Blocker owner: {owner}."));
            }
            let (pair, changed) = pair(
                &format!("specs:goal:{}", goal.id),
                &format!(
                    "Abbey goal {}: what is it, what is its status, and what is its next action?",
                    goal.id
                ),
                &response,
            );
            report.redacted_pairs += usize::from(changed);
            pairs.push(pair);
        }
        report.goals = goals.len();
    }

    (pairs, report)
}

/// One redacted pair, and whether the redaction changed it.
fn pair(id: &str, prompt: &str, response: &str) -> (Pair, bool) {
    let prompt = redact(prompt);
    let response = redact(response);
    let changed = prompt.changed || response.changed;
    (
        Pair {
            id: id.to_string(),
            prompt: prompt.text,
            response: response.text,
        },
        changed,
    )
}

/// Parse the goal ledger, skipping malformed blocks.
///
/// This mirrors the shape `src/improve/ledger.rs` parses, but it is
/// deliberately tolerant: a training corpus must not abort because one goal
/// block is malformed. The number of skipped blocks is reported instead, so
/// the omission is visible rather than silent.
#[must_use]
pub fn parse_goals(text: &str) -> (Vec<GoalSpec>, usize) {
    let lines: Vec<&str> = text.lines().collect();
    let mut goals = Vec::new();
    let mut skipped = 0usize;
    let mut index = 0usize;
    while index < lines.len() {
        let Some(title) = lines[index].strip_prefix("## ") else {
            index += 1;
            continue;
        };
        let title = title.trim().to_string();
        index += 1;
        if lines.get(index).map(|line| line.trim()) != Some("<!-- abbey-goal") {
            continue;
        }
        index += 1;
        let mut fields: Vec<(String, String)> = Vec::new();
        let mut closed = false;
        while index < lines.len() {
            let line = lines[index].trim();
            if line == "-->" {
                closed = true;
                index += 1;
                break;
            }
            if !line.is_empty()
                && let Some((key, value)) = line.split_once(':')
            {
                fields.push((key.trim().to_ascii_lowercase(), value.trim().to_string()));
            }
            index += 1;
        }
        if !closed {
            skipped += 1;
            continue;
        }
        let lookup = |name: &str| {
            fields
                .iter()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.clone())
                .filter(|value| !value.is_empty())
        };
        let (Some(id), Some(status)) = (lookup("id"), lookup("status")) else {
            skipped += 1;
            continue;
        };
        goals.push(GoalSpec {
            id,
            title,
            status,
            implementation: lookup("implementation-evidence").unwrap_or_default(),
            automated_test: lookup("automated-test-evidence").unwrap_or_default(),
            live_external: lookup("live-external-evidence").unwrap_or_default(),
            next_action: lookup("next-action").unwrap_or_default(),
            blocker_owner: lookup("blocker-owner"),
        });
    }
    (goals, skipped)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_claim_registry_becomes_one_pair_per_claim() {
        let (pairs, report) = corpus(None);
        assert_eq!(report.claims, CLAIMS.len());
        assert_eq!(pairs.len(), CLAIMS.len());
        assert!(pairs.iter().all(|p| p.id.starts_with("specs:claim:")));
        assert!(
            pairs
                .iter()
                .all(|p| !p.prompt.is_empty() && !p.response.is_empty())
        );
        assert!(
            report.redacted_pairs <= pairs.len(),
            "the redaction count cannot exceed the corpus"
        );
    }

    #[test]
    fn a_specs_pair_with_an_absolute_path_is_redacted_and_counted() {
        let dir = std::env::temp_dir().join(format!("abbey-train-specs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("goals.md");
        std::fs::write(
            &path,
            "## Fixture goal\n<!-- abbey-goal\nid: fixture-goal\nstatus: done\n\
             implementation-evidence: the binary lives at /usr/local/bin/abbey\n\
             automated-test-evidence: none\nlive-external-evidence: none\nnext-action: none\n-->\n",
        )
        .unwrap();
        let (pairs, report) = corpus(Some(&path));
        let (_, base) = corpus(None);
        assert_eq!(report.goals, 1);
        assert_eq!(report.redacted_pairs, base.redacted_pairs + 1);
        let goal = pairs
            .iter()
            .find(|pair| pair.id == "specs:goal:fixture-goal")
            .expect("the goal pair");
        assert!(
            goal.response.contains("[path]"),
            "the absolute path must be redacted: {}",
            goal.response
        );
        assert!(!goal.response.contains("/usr/local/bin"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_malformed_goal_block_is_skipped_and_counted() {
        let text = "\
## One goal
<!-- abbey-goal
id: one
status: done
next-action: none
-->
body

## Broken
<!-- abbey-goal
id: two
-->
";
        let (goals, skipped) = parse_goals(text);
        assert_eq!(goals.len(), 1);
        assert_eq!(goals[0].id, "one");
        assert_eq!(goals[0].status, "done");
        assert_eq!(skipped, 1);
    }

    #[test]
    fn the_real_goal_ledger_parses_without_skips() {
        let path = std::path::Path::new("tasks/goals.md");
        if !path.exists() {
            return;
        }
        let text = std::fs::read_to_string(path).unwrap();
        let (goals, skipped) = parse_goals(&text);
        assert!(!goals.is_empty());
        assert_eq!(skipped, 0, "the shipped goal ledger must parse cleanly");
    }
}
