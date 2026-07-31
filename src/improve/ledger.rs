//! Goal ledger + todo checklist parsing (`tasks/goals.md`, `tasks/todo.md`).
//!
//! Tolerant markdown: `## Title`, `status: todo|in_progress|blocked|done`,
//! and `- [ ]` / `- [x]` checkboxes. Abbey never auto-rewrites `goals.md`
//! status — that stays a human/session close after evidence.

use anyhow::{Result, bail};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GoalStatus {
    Todo,
    InProgress,
    Blocked,
    Done,
}

impl GoalStatus {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "todo" => Some(Self::Todo),
            "in_progress" | "in-progress" | "wip" => Some(Self::InProgress),
            "blocked" => Some(Self::Blocked),
            "done" | "closed" | "complete" => Some(Self::Done),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Todo => "todo",
            Self::InProgress => "in_progress",
            Self::Blocked => "blocked",
            Self::Done => "done",
        }
    }

    pub fn is_open(self) -> bool {
        matches!(self, Self::Todo | Self::InProgress | Self::Blocked)
    }
}

#[derive(Debug, Clone)]
pub struct Goal {
    pub title: String,
    pub status: GoalStatus,
    pub body: String,
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
            goals: parse_goals(&goals_text),
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

    pub fn open_todo_count(&self) -> usize {
        self.open_todos().count()
    }

    /// Prefer `in_progress`, else first `todo`, else `blocked`.
    pub fn pick_goal(&self, hint: Option<&str>) -> Option<&Goal> {
        if let Some(h) = hint {
            let key = h.trim().to_ascii_lowercase();
            if !key.is_empty() {
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
            }
        }
        self.goals
            .iter()
            .find(|g| g.status == GoalStatus::InProgress)
            .or_else(|| self.goals.iter().find(|g| g.status == GoalStatus::Todo))
            .or_else(|| self.goals.iter().find(|g| g.status == GoalStatus::Blocked))
    }

    pub fn next_open_todo(&self) -> Option<&TodoItem> {
        self.open_todos().next()
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
pub fn pick_work(ledger: &Ledger, hint: Option<&str>, gate_only: bool) -> WorkFocus {
    if gate_only {
        return WorkFocus::Stabilize;
    }
    if let Some(g) = ledger.pick_goal(hint) {
        let todo = ledger.next_open_todo().map(|t| t.text.clone());
        return WorkFocus::Goal {
            title: g.title.clone(),
            status: g.status,
            body: g.body.clone(),
            todo,
        };
    }
    if let Some(t) = ledger.next_open_todo() {
        return WorkFocus::Goal {
            title: "open todo (no open goal)".into(),
            status: GoalStatus::InProgress,
            body: String::new(),
            todo: Some(t.text.clone()),
        };
    }
    WorkFocus::Stabilize
}

pub fn parse_goals(text: &str) -> Vec<Goal> {
    let mut goals = Vec::new();
    let mut title: Option<String> = None;
    let mut status = GoalStatus::Todo;
    let mut body = String::new();

    let flush = |goals: &mut Vec<Goal>,
                 title: &mut Option<String>,
                 status: &mut GoalStatus,
                 body: &mut String| {
        if let Some(t) = title.take() {
            goals.push(Goal {
                title: t,
                status: *status,
                body: body.trim().to_string(),
            });
            *status = GoalStatus::Todo;
            body.clear();
        }
    };

    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("## ") {
            flush(&mut goals, &mut title, &mut status, &mut body);
            title = Some(rest.trim().to_string());
            continue;
        }
        if title.is_none() {
            continue;
        }
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("status:") {
            if let Some(s) = GoalStatus::parse(rest) {
                status = s;
            }
            continue;
        }
        body.push_str(line);
        body.push('\n');
    }
    flush(&mut goals, &mut title, &mut status, &mut body);
    goals
}

/// Whether a `todo.md` heading marks capabilities Abbey deliberately does not
/// build (Proposed / Out of scope), rather than pending work.
///
/// Matched on the section heading rather than item text so that a legitimate
/// todo like "document why X is out of scope" is still picked up as work.
pub fn is_deferred_section(section: &str) -> bool {
    let s = section.to_ascii_lowercase();
    s.contains("deferred") || s.contains("out of scope") || s.contains("not abbey current")
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
mod tests {
    use super::*;

    #[test]
    fn parse_goals_statuses() {
        let text = r#"
# Goals

## Ship A
status: done
- landed

## Ship B
status: in_progress
- still open

## Ship C
status: todo
"#;
        let g = parse_goals(text);
        assert_eq!(g.len(), 3);
        assert_eq!(g[0].status, GoalStatus::Done);
        assert_eq!(g[1].status, GoalStatus::InProgress);
        assert_eq!(g[2].status, GoalStatus::Todo);
        assert!(g[1].body.contains("still open"));
    }

    #[test]
    fn pick_prefers_in_progress() {
        let ledger = Ledger {
            goals_path: PathBuf::from("x"),
            todo_path: PathBuf::from("y"),
            goals: vec![
                Goal {
                    title: "Old".into(),
                    status: GoalStatus::Todo,
                    body: String::new(),
                },
                Goal {
                    title: "Active".into(),
                    status: GoalStatus::InProgress,
                    body: String::new(),
                },
            ],
            todos: vec![TodoItem {
                text: "do the thing".into(),
                done: false,
                section: "Phase".into(),
            }],
        };
        let w = pick_work(&ledger, None, false);
        match w {
            WorkFocus::Goal { title, todo, .. } => {
                assert_eq!(title, "Active");
                assert_eq!(todo.as_deref(), Some("do the thing"));
            }
            WorkFocus::Stabilize => panic!("expected goal"),
        }
    }

    #[test]
    fn all_done_or_gate_only_stabilizes() {
        let ledger = Ledger {
            goals_path: PathBuf::new(),
            todo_path: PathBuf::new(),
            goals: vec![Goal {
                title: "Closed".into(),
                status: GoalStatus::Done,
                body: String::new(),
            }],
            todos: vec![TodoItem {
                text: "done item".into(),
                done: true,
                section: String::new(),
            }],
        };
        assert!(matches!(
            pick_work(&ledger, None, false),
            WorkFocus::Stabilize
        ));
        assert!(matches!(
            pick_work(&ledger, None, true),
            WorkFocus::Stabilize
        ));
    }

    #[test]
    fn deferred_sections_are_never_nominated_as_work() {
        // Regression: an unchecked box under a deferred heading was picked as
        // the next slice, so `improve run --confirm` would dispatch Max to
        // build something `abbey claims refuse` exits 2 for.
        let text = "## Deferred by construction (Proposed / Out of scope — not Abbey Current)\n\
                    - [ ] Semantic memory search (learned embedding space) — **Proposed**\n\
                    - [ ] Multi-node / multi-GPU mesh — **Proposed**\n";
        let todos = parse_todos(text);
        assert_eq!(todos.len(), 2, "parser still sees the items");

        let ledger = Ledger {
            goals_path: PathBuf::new(),
            todo_path: PathBuf::new(),
            goals: vec![Goal {
                title: "Closed".into(),
                status: GoalStatus::Done,
                body: String::new(),
            }],
            todos,
        };
        assert_eq!(ledger.open_todo_count(), 0, "deferred items are not work");
        assert!(ledger.next_open_todo().is_none());
        assert!(matches!(
            pick_work(&ledger, None, false),
            WorkFocus::Stabilize
        ));
    }

    #[test]
    fn a_real_todo_mentioning_out_of_scope_is_still_work() {
        // Filtering on the heading, not item text, so this stays pickable.
        let text = "## Docs\n- [ ] document why LoRA is out of scope\n";
        let ledger = Ledger {
            goals_path: PathBuf::new(),
            todo_path: PathBuf::new(),
            goals: Vec::new(),
            todos: parse_todos(text),
        };
        assert_eq!(ledger.open_todo_count(), 1);
    }

    #[test]
    fn parse_todo_checkboxes() {
        let text = "## Hybrid\n- [x] done\n- [ ] open\n### Nested\n- [ ] nested open\n";
        let t = parse_todos(text);
        assert_eq!(t.len(), 3);
        assert!(t[0].done);
        assert!(!t[1].done);
        assert_eq!(t[1].text, "open");
        assert_eq!(t[2].section, "Nested");
    }
}
