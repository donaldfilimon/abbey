//! Goal ledger + todo checklist parsing (`tasks/goals.md`, `tasks/todo.md`).
//!
//! Goals use `## Title` followed immediately by a machine-readable
//! `<!-- abbey-goal` metadata block. Todo checkboxes remain tolerant markdown:
//! `- [ ]` / `- [x]`. Abbey never auto-rewrites `goals.md` status — that stays
//! a human/session close after evidence.

use anyhow::{Result, bail};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GoalStatus {
    Todo,
    InProgress,
    Blocked,
    Proposed,
    Done,
}

impl GoalStatus {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "todo" => Some(Self::Todo),
            "in_progress" => Some(Self::InProgress),
            "blocked" => Some(Self::Blocked),
            "proposed" => Some(Self::Proposed),
            "done" => Some(Self::Done),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Todo => "todo",
            Self::InProgress => "in_progress",
            Self::Blocked => "blocked",
            Self::Proposed => "proposed",
            Self::Done => "done",
        }
    }

    pub fn is_open(self) -> bool {
        matches!(
            self,
            Self::Todo | Self::InProgress | Self::Blocked | Self::Proposed
        )
    }
}

#[derive(Debug, Clone)]
pub struct Goal {
    /// Stable machine identifier. Titles may change; this must not.
    pub id: String,
    pub title: String,
    pub status: GoalStatus,
    pub implementation_evidence: String,
    pub automated_test_evidence: String,
    pub live_external_evidence: String,
    pub next_action: String,
    pub blocker_owner: Option<String>,
    pub body: String,
}

impl Goal {
    /// Claims-bearing context supplied to improve lanes. Keeping the typed
    /// metadata ahead of the narrative body prevents a long goal description
    /// from hiding its evidence boundary or concrete next action.
    fn runtime_context(&self) -> String {
        let blocker_owner = self.blocker_owner.as_deref().unwrap_or("none");
        let mut context = format!(
            "Goal ID: {}\nImplementation evidence: {}\nAutomated-test evidence: {}\n\
             Live/external evidence: {}\nNext action: {}\nBlocker owner: {}",
            self.id,
            self.implementation_evidence,
            self.automated_test_evidence,
            self.live_external_evidence,
            self.next_action,
            blocker_owner,
        );
        if !self.body.is_empty() {
            context.push_str("\n\n");
            context.push_str(&self.body);
        }
        context
    }
}

#[derive(Debug, Clone)]
pub struct TodoItem {
    pub text: String,
    pub done: bool,
    pub section: String,
}

#[derive(Debug, Clone)]
pub struct Ledger {
    /// The exact files this ledger was loaded from. `improve status` prints
    /// them, which is what tells you *which* path was empty when a ledger
    /// looks unexpectedly bare.
    pub goals_path: PathBuf,
    pub todo_path: PathBuf,
    pub goals: Vec<Goal>,
    pub todos: Vec<TodoItem>,
}

impl Ledger {
    pub fn load(root: &Path) -> Result<Self> {
        let goals_path = root.join("tasks/goals.md");
        let todo_path = root.join("tasks/todo.md");
        let goals_text = if goals_path.exists() {
            fs::read_to_string(&goals_path)?
        } else {
            String::new()
        };
        let todo_text = if todo_path.exists() {
            fs::read_to_string(&todo_path)?
        } else {
            String::new()
        };
        Ok(Self {
            goals_path,
            todo_path,
            goals: parse_goals(&goals_text)?,
            todos: parse_todos(&todo_text),
        })
    }

    pub fn open_goals(&self) -> impl Iterator<Item = &Goal> {
        self.goals.iter().filter(|g| g.status.is_open())
    }

    /// Unchecked todos that are actually *work* — deferred-by-construction
    /// sections are excluded.
    ///
    /// Without this, an unchecked box under "Deferred by construction" was
    /// nominated as the next slice, so `improve run --confirm` would dispatch
    /// Max to build a capability `abbey claims refuse` exits 2 for. The
    /// checkbox format alone is not enough — it regresses the moment someone
    /// writes `- [ ]` under that heading again.
    pub fn open_todos(&self) -> impl Iterator<Item = &TodoItem> {
        self.todos
            .iter()
            .filter(|t| !t.done && !is_deferred_section(&t.section))
    }

    /// Unchecked todos Abbey can act on without an external state change.
    ///
    /// Blocked items remain visible through [`Self::open_todos`] and status
    /// output, but the autonomous improve loop must not spend agent rounds on
    /// account, billing, hosted-runner, or other explicitly blocked sections.
    pub fn actionable_todos(&self) -> impl Iterator<Item = &TodoItem> {
        self.open_todos()
            .filter(|t| !is_blocked_section(&t.section))
    }

    pub fn open_todo_count(&self) -> usize {
        self.open_todos().count()
    }

    pub fn actionable_todo_count(&self) -> usize {
        self.actionable_todos().count()
    }

    /// Prefer actionable goals by default; an explicit ID/title hint may
    /// select a blocked or proposed goal for inspection or a deliberate retry.
    pub fn pick_goal(&self, hint: Option<&str>) -> Option<&Goal> {
        if let Some(h) = hint {
            let key = h.trim().to_ascii_lowercase();
            if !key.is_empty() {
                if let Some(g) = self.goals.iter().find(|g| g.id.to_ascii_lowercase() == key) {
                    return Some(g);
                }
                if let Some(g) = self
                    .goals
                    .iter()
                    .find(|g| g.title.to_ascii_lowercase().contains(&key) && g.status.is_open())
                {
                    return Some(g);
                }
                if let Some(g) = self
                    .goals
                    .iter()
                    .find(|g| g.title.to_ascii_lowercase().contains(&key))
                {
                    return Some(g);
                }
                return None;
            }
        }
        self.default_goal()
    }

    fn default_goal(&self) -> Option<&Goal> {
        self.goals
            .iter()
            .find(|g| g.status == GoalStatus::InProgress)
            .or_else(|| self.goals.iter().find(|g| g.status == GoalStatus::Todo))
    }

    pub fn next_open_todo(&self) -> Option<&TodoItem> {
        self.open_todos().next()
    }

    pub fn next_actionable_todo(&self) -> Option<&TodoItem> {
        if let Some(goal) = self.default_goal()
            && let Some(todo) = self
                .actionable_todos()
                .find(|todo| todo.text.contains(goal.next_action.trim()))
        {
            return Some(todo);
        }
        self.actionable_todos().next()
    }

    /// Resolve the selected goal's declared next action against autonomous
    /// work before falling back to the first actionable checklist item.
    fn actionable_todo_for(&self, goal: &Goal) -> Option<&TodoItem> {
        self.actionable_todos()
            .find(|todo| todo.text.contains(goal.next_action.trim()))
            .or_else(|| self.next_actionable_todo())
    }

    /// Explicit inspection may include blocked work, but Proposed/Done goals
    /// must not inherit an unrelated open TODO merely because none matches.
    fn open_todo_for(&self, goal: &Goal) -> Option<&TodoItem> {
        if let Some(matching) = self
            .open_todos()
            .find(|todo| todo.text.contains(goal.next_action.trim()))
        {
            return Some(matching);
        }
        if matches!(
            goal.status,
            GoalStatus::Todo | GoalStatus::InProgress | GoalStatus::Blocked
        ) {
            self.next_open_todo()
        } else {
            None
        }
    }
}

/// What the improve loop should work on this run.
#[derive(Debug, Clone)]
pub enum WorkFocus {
    /// Heal `./check.sh` only (gate red / `--gate-only` / ledger already closed).
    Stabilize,
    /// Drive an open goal (+ optional unchecked todo).
    Goal {
        title: String,
        status: GoalStatus,
        body: String,
        todo: Option<String>,
    },
}

impl WorkFocus {
    pub fn summary(&self) -> String {
        match self {
            Self::Stabilize => "stabilize — make ./check.sh green".into(),
            Self::Goal {
                title,
                status,
                todo,
                ..
            } => match todo {
                Some(t) => format!("goal `{title}` ({}) · todo: {t}", status.label()),
                None => format!("goal `{title}` ({})", status.label()),
            },
        }
    }
}

/// Pick work: gate-only → stabilize; else open goal/todo; else stabilize when
/// ledger closed (or empty) so a red gate still has something to heal.
pub fn pick_work(ledger: &Ledger, hint: Option<&str>, gate_only: bool) -> Result<WorkFocus> {
    let goal = ledger.pick_goal(hint);
    if goal.is_none() && hint.is_some_and(|value| !value.trim().is_empty()) {
        bail!(
            "improve: unknown goal hint `{}`",
            hint.unwrap_or_default().trim()
        );
    }
    if gate_only {
        return Ok(WorkFocus::Stabilize);
    }
    if let Some(g) = goal {
        let todo = if hint.is_some() {
            ledger.open_todo_for(g)
        } else {
            ledger.actionable_todo_for(g)
        }
        .map(|t| t.text.clone());
        return Ok(WorkFocus::Goal {
            title: g.title.clone(),
            status: g.status,
            body: g.runtime_context(),
            todo,
        });
    }
    if let Some(t) = ledger.next_actionable_todo() {
        return Ok(WorkFocus::Goal {
            title: "open todo (no open goal)".into(),
            status: GoalStatus::InProgress,
            body: String::new(),
            todo: Some(t.text.clone()),
        });
    }
    Ok(WorkFocus::Stabilize)
}

const GOAL_METADATA_OPEN: &str = "<!-- abbey-goal";
const GOAL_METADATA_CLOSE: &str = "-->";

#[derive(Default)]
struct GoalMetadata {
    id: Option<String>,
    status: Option<GoalStatus>,
    implementation_evidence: Option<String>,
    automated_test_evidence: Option<String>,
    live_external_evidence: Option<String>,
    next_action: Option<String>,
    blocker_owner: Option<String>,
}

/// Parse the claims-bearing goal ledger.
///
/// Every `##` goal heading must be followed on the very next line by:
///
/// ```text
/// <!-- abbey-goal
/// id: stable-kebab-case-id
/// status: todo|in_progress|blocked|proposed|done
/// implementation-evidence: concise source/commit evidence or none
/// automated-test-evidence: concise automated evidence or none
/// live-external-evidence: concise live/external evidence or none
/// next-action: concrete next action or none when closed
/// blocker-owner: required for blocked goals; optional otherwise
/// -->
/// ```
///
/// Unknown keys, duplicate keys/IDs, missing values, and unknown statuses are
/// errors. This is deliberately fail-closed: malformed claims never become
/// actionable `todo` work by accident.
pub fn parse_goals(text: &str) -> Result<Vec<Goal>> {
    let lines: Vec<&str> = text.lines().collect();
    let mut goals = Vec::new();
    let mut ids = HashSet::new();
    let mut index = 0;

    while index < lines.len() {
        let Some(title_text) = lines[index].strip_prefix("## ") else {
            index += 1;
            continue;
        };
        let heading_line = index + 1;
        let title = title_text.trim();
        if title.is_empty() {
            bail!("goals.md:{heading_line}: goal heading must have a title");
        }

        index += 1;
        if lines.get(index).map(|line| line.trim()) != Some(GOAL_METADATA_OPEN) {
            bail!(
                "goals.md:{heading_line}: goal `{title}` must be followed immediately by `{GOAL_METADATA_OPEN}`"
            );
        }
        index += 1;

        let mut metadata = GoalMetadata::default();
        let mut seen_keys = HashSet::new();
        let mut closed = false;
        while index < lines.len() {
            let line_number = index + 1;
            let line = lines[index].trim();
            if line == GOAL_METADATA_CLOSE {
                closed = true;
                index += 1;
                break;
            }
            if line.is_empty() {
                bail!(
                    "goals.md:{line_number}: blank lines are not allowed in goal `{title}` metadata"
                );
            }
            let Some((key, value)) = line.split_once(':') else {
                bail!(
                    "goals.md:{line_number}: malformed goal `{title}` metadata; expected `key: value`"
                );
            };
            let key = key.trim();
            let value = value.trim();
            if key.is_empty() || value.is_empty() {
                bail!(
                    "goals.md:{line_number}: goal `{title}` metadata key and value must be nonempty"
                );
            }
            if !seen_keys.insert(key.to_string()) {
                bail!("goals.md:{line_number}: duplicate `{key}` metadata for goal `{title}`");
            }
            match key {
                "id" => metadata.id = Some(value.to_string()),
                "status" => {
                    metadata.status = Some(GoalStatus::parse(value).ok_or_else(|| {
                        anyhow::anyhow!(
                            "goals.md:{line_number}: unknown status `{value}` for goal `{title}`"
                        )
                    })?);
                }
                "implementation-evidence" => {
                    metadata.implementation_evidence = Some(value.to_string());
                }
                "automated-test-evidence" => {
                    metadata.automated_test_evidence = Some(value.to_string());
                }
                "live-external-evidence" => {
                    metadata.live_external_evidence = Some(value.to_string());
                }
                "next-action" => metadata.next_action = Some(value.to_string()),
                "blocker-owner" => metadata.blocker_owner = Some(value.to_string()),
                _ => {
                    bail!("goals.md:{line_number}: unknown metadata key `{key}` for goal `{title}`")
                }
            }
            index += 1;
        }
        if !closed {
            bail!("goals.md:{heading_line}: unclosed metadata for goal `{title}`");
        }

        let id = require_metadata(metadata.id, "id", title, heading_line)?;
        if !is_valid_goal_id(&id) {
            bail!(
                "goals.md:{heading_line}: invalid goal id `{id}`; expected lowercase ASCII kebab-case"
            );
        }
        if !ids.insert(id.clone()) {
            bail!("goals.md:{heading_line}: duplicate goal id `{id}`");
        }
        let status = metadata.status.ok_or_else(|| {
            anyhow::anyhow!("goals.md:{heading_line}: goal `{title}` is missing `status`")
        })?;
        let implementation_evidence = require_metadata(
            metadata.implementation_evidence,
            "implementation-evidence",
            title,
            heading_line,
        )?;
        let automated_test_evidence = require_metadata(
            metadata.automated_test_evidence,
            "automated-test-evidence",
            title,
            heading_line,
        )?;
        let live_external_evidence = require_metadata(
            metadata.live_external_evidence,
            "live-external-evidence",
            title,
            heading_line,
        )?;
        let next_action =
            require_metadata(metadata.next_action, "next-action", title, heading_line)?;
        if status == GoalStatus::Blocked && metadata.blocker_owner.is_none() {
            bail!("goals.md:{heading_line}: blocked goal `{title}` is missing `blocker-owner`");
        }

        let body_start = index;
        while index < lines.len() && !lines[index].starts_with("## ") {
            index += 1;
        }
        let body = lines[body_start..index].join("\n").trim().to_string();
        goals.push(Goal {
            id,
            title: title.to_string(),
            status,
            implementation_evidence,
            automated_test_evidence,
            live_external_evidence,
            next_action,
            blocker_owner: metadata.blocker_owner,
            body,
        });
    }

    Ok(goals)
}

fn require_metadata(
    value: Option<String>,
    key: &str,
    title: &str,
    heading_line: usize,
) -> Result<String> {
    value.ok_or_else(|| {
        anyhow::anyhow!("goals.md:{heading_line}: goal `{title}` is missing `{key}`")
    })
}

fn is_valid_goal_id(id: &str) -> bool {
    !id.is_empty()
        && !id.starts_with('-')
        && !id.ends_with('-')
        && !id.contains("--")
        && id
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

/// Whether a `todo.md` heading marks capabilities Abbey deliberately does not
/// build (Proposed / Out of scope), rather than pending work.
///
/// Matched on the section heading rather than item text so that a legitimate
/// todo like "document why X is out of scope" is still picked up as work.
pub fn is_deferred_section(section: &str) -> bool {
    let s = section.to_ascii_lowercase();
    s.contains("deferred")
        || s.contains("proposed")
        || s.contains("out of scope")
        || s.contains("not abbey current")
}

/// Whether a todo section records an external blocker rather than locally
/// executable work. Blocked items stay in the ledger and status counts.
pub fn is_blocked_section(section: &str) -> bool {
    section.to_ascii_lowercase().contains("blocked")
}

pub fn parse_todos(text: &str) -> Vec<TodoItem> {
    let mut items = Vec::new();
    let mut section = String::new();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("## ") {
            section = rest.trim().to_string();
            continue;
        }
        if let Some(rest) = line.strip_prefix("### ") {
            section = rest.trim().to_string();
            continue;
        }
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("- [ ] ") {
            items.push(TodoItem {
                text: rest.trim().to_string(),
                done: false,
                section: section.clone(),
            });
        } else if let Some(rest) = t
            .strip_prefix("- [x] ")
            .or_else(|| t.strip_prefix("- [X] "))
        {
            items.push(TodoItem {
                text: rest.trim().to_string(),
                done: true,
                section: section.clone(),
            });
        }
    }
    items
}

pub fn require_tasks_root(cwd: &Path) -> Result<PathBuf> {
    let candidates = [cwd.to_path_buf(), cwd.join("..")];
    for c in &candidates {
        if c.join("tasks/goals.md").exists() || c.join("check.sh").exists() {
            return Ok(c.canonicalize().unwrap_or_else(|_| c.clone()));
        }
    }
    if cwd.join("Cargo.toml").exists() {
        return Ok(cwd.to_path_buf());
    }
    bail!("improve: no tasks/ or check.sh under {}", cwd.display())
}

#[cfg(test)]
#[path = "ledger/tests.rs"]
mod strict_tests;

#[cfg(test)]
#[path = "ledger/unit_tests.rs"]
mod tests;
