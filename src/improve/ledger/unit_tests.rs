use super::*;

fn goal(title: &str, status: GoalStatus) -> Goal {
    Goal {
        id: title.to_ascii_lowercase().replace(' ', "-"),
        title: title.into(),
        status,
        implementation_evidence: "source evidence".into(),
        automated_test_evidence: "unit test evidence".into(),
        live_external_evidence: "not required".into(),
        next_action: "none".into(),
        blocker_owner: (status == GoalStatus::Blocked).then_some("repository owner".into()),
        body: String::new(),
    }
}

#[test]
fn parses_current_goal_metadata_and_body() {
    let text = r#"
# Goals

## Ship A
<!-- abbey-goal
id: ship-a
status: done
implementation-evidence: src/a.rs at commit abc123
automated-test-evidence: cargo test ship_a passes
live-external-evidence: live smoke passed 2026-08-08
next-action: none; closed
-->
- landed

## Ship B
<!-- abbey-goal
id: ship-b
status: in_progress
implementation-evidence: partial implementation in src/b.rs
automated-test-evidence: parser tests pass
live-external-evidence: pending
next-action: finish runtime wiring
-->
- still open

## Ship C
<!-- abbey-goal
id: ship-c
status: todo
implementation-evidence: none
automated-test-evidence: none
live-external-evidence: none
next-action: implement first slice
-->
"#;
    let g = parse_goals(text).expect("valid metadata");
    assert_eq!(g.len(), 3);
    assert_eq!(g[0].id, "ship-a");
    assert_eq!(g[0].status, GoalStatus::Done);
    assert_eq!(g[0].implementation_evidence, "src/a.rs at commit abc123");
    assert_eq!(g[0].automated_test_evidence, "cargo test ship_a passes");
    assert_eq!(g[0].live_external_evidence, "live smoke passed 2026-08-08");
    assert_eq!(g[0].next_action, "none; closed");
    assert_eq!(g[0].blocker_owner, None);
    assert_eq!(g[1].status, GoalStatus::InProgress);
    assert_eq!(g[2].status, GoalStatus::Todo);
    assert!(g[1].body.contains("still open"));
}

#[test]
fn proposed_is_visible_but_not_picked_by_default() {
    let parsed = parse_goals(
        r#"## Future runtime
<!-- abbey-goal
id: future-runtime
status: proposed
implementation-evidence: none
automated-test-evidence: none
live-external-evidence: none
next-action: await roadmap approval
-->
"#,
    )
    .expect("proposed is a valid distinct status");
    assert_eq!(parsed[0].status, GoalStatus::Proposed);

    let ledger = Ledger {
        goals_path: PathBuf::new(),
        todo_path: PathBuf::new(),
        goals: parsed,
        todos: Vec::new(),
    };

    assert_eq!(ledger.open_goals().count(), 1);
    assert!(ledger.pick_goal(None).is_none());
    assert!(matches!(
        pick_work(&ledger, None, false).expect("default selection"),
        WorkFocus::Stabilize
    ));

    let explicit = ledger
        .pick_goal(Some("future-runtime"))
        .expect("explicit stable ID remains inspectable");
    assert_eq!(explicit.status, GoalStatus::Proposed);
}

#[test]
fn duplicate_and_missing_goal_ids_are_rejected() {
    let duplicate = r#"## One
<!-- abbey-goal
id: same-id
status: done
implementation-evidence: shipped
automated-test-evidence: tested
live-external-evidence: verified
next-action: none
-->
## Two
<!-- abbey-goal
id: same-id
status: todo
implementation-evidence: none
automated-test-evidence: none
live-external-evidence: none
next-action: start
-->
"#;
    let error = parse_goals(duplicate).expect_err("duplicate IDs must fail closed");
    assert!(error.to_string().contains("duplicate goal id `same-id`"));

    let missing = r#"## Missing ID
<!-- abbey-goal
status: todo
implementation-evidence: none
automated-test-evidence: none
live-external-evidence: none
next-action: start
-->
"#;
    let error = parse_goals(missing).expect_err("missing ID must fail closed");
    assert!(error.to_string().contains("missing `id`"));

    let empty = missing.replacen("status: todo", "id:\nstatus: todo", 1);
    assert!(
        parse_goals(&empty)
            .expect_err("empty ID must fail closed")
            .to_string()
            .contains("key and value must be nonempty")
    );
}

#[test]
fn missing_malformed_and_unknown_statuses_are_rejected() {
    let missing = r#"## No status
<!-- abbey-goal
id: no-status
implementation-evidence: none
automated-test-evidence: none
live-external-evidence: none
next-action: decide status
-->
"#;
    assert!(
        parse_goals(missing)
            .expect_err("missing status must fail closed")
            .to_string()
            .contains("missing `status`")
    );

    let unknown = missing.replace("id: no-status", "id: bad-status\nstatus: someday");
    assert!(
        parse_goals(&unknown)
            .expect_err("unknown status must fail closed")
            .to_string()
            .contains("unknown status `someday`")
    );

    let malformed = missing.replace("id: no-status", "id: no-status\nstatus todo");
    assert!(
        parse_goals(&malformed)
            .expect_err("malformed status must fail closed")
            .to_string()
            .contains("expected `key: value`")
    );

    let alias = missing.replace("id: no-status", "id: alias\nstatus: wip");
    assert!(
        parse_goals(&alias)
            .expect_err("noncanonical status must fail closed")
            .to_string()
            .contains("unknown status `wip`")
    );
}

#[test]
fn incomplete_metadata_and_blocked_without_owner_are_rejected() {
    let missing_evidence = r#"## Incomplete
<!-- abbey-goal
id: incomplete
status: todo
implementation-evidence: none
live-external-evidence: none
next-action: add tests
-->
"#;
    assert!(
        parse_goals(missing_evidence)
            .expect_err("all evidence fields are mandatory")
            .to_string()
            .contains("missing `automated-test-evidence`")
    );

    let blocked_without_owner = r#"## Externally blocked
<!-- abbey-goal
id: external-block
status: blocked
implementation-evidence: local slice shipped
automated-test-evidence: local tests pass
live-external-evidence: hosted proof pending
next-action: owner provisions runner
-->
"#;
    assert!(
        parse_goals(blocked_without_owner)
            .expect_err("blocked goals require an owner")
            .to_string()
            .contains("missing `blocker-owner`")
    );
}

#[test]
fn metadata_must_be_immediately_below_the_heading() {
    let text = "## Delayed\n\n<!-- abbey-goal\n";
    assert!(
        parse_goals(text)
            .expect_err("detached metadata must be rejected")
            .to_string()
            .contains("must be followed immediately")
    );
}

#[test]
fn pick_prefers_in_progress() {
    let ledger = Ledger {
        goals_path: PathBuf::from("x"),
        todo_path: PathBuf::from("y"),
        goals: vec![
            goal("Old", GoalStatus::Todo),
            goal("Active", GoalStatus::InProgress),
        ],
        todos: vec![TodoItem {
            text: "do the thing".into(),
            done: false,
            section: "Phase".into(),
        }],
    };
    let w = pick_work(&ledger, None, false).expect("default selection");
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
        goals: vec![goal("Closed", GoalStatus::Done)],
        todos: vec![TodoItem {
            text: "done item".into(),
            done: true,
            section: String::new(),
        }],
    };
    assert!(matches!(
        pick_work(&ledger, None, false).expect("default selection"),
        WorkFocus::Stabilize
    ));
    assert!(matches!(
        pick_work(&ledger, None, true).expect("gate-only selection"),
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
        goals: vec![goal("Closed", GoalStatus::Done)],
        todos,
    };
    assert_eq!(ledger.open_todo_count(), 0, "deferred items are not work");
    assert!(ledger.next_open_todo().is_none());
    assert!(matches!(
        pick_work(&ledger, None, false).expect("default selection"),
        WorkFocus::Stabilize
    ));
}

#[test]
fn next_action_skips_earlier_work_and_proposed_todos_do_not_leak() {
    let current = Goal {
        next_action: "Phase 3 — Claims and ledgers as executable specifications".into(),
        ..goal("Current program", GoalStatus::InProgress)
    };
    let ledger = Ledger {
        goals_path: PathBuf::new(),
        todo_path: PathBuf::new(),
        goals: vec![goal("Future runtime", GoalStatus::Proposed), current],
        todos: parse_todos(
            "## Approved runtime roadmap (Proposed, not Current)\n\
                 - [ ] build the proposed runtime\n\
                 ## Current program\n\
                 - [ ] **Phase 2 — Self-hosted runners replace the broken hosted-CI assumption**\n\
                 - [ ] **Phase 3 — Claims and ledgers as executable specifications**\n",
        ),
    };

    assert_eq!(ledger.open_todo_count(), 2);
    assert_eq!(
        ledger.next_actionable_todo().map(|todo| todo.text.as_str()),
        Some("**Phase 3 — Claims and ledgers as executable specifications**")
    );
    let focus = pick_work(&ledger, None, false).expect("default selection");
    match focus {
        WorkFocus::Goal { title, todo, .. } => {
            assert_eq!(title, "Current program");
            assert_eq!(
                todo.as_deref(),
                Some("**Phase 3 — Claims and ledgers as executable specifications**")
            );
        }
        WorkFocus::Stabilize => panic!("current work should remain actionable"),
    }
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
fn blocked_work_stays_visible_but_is_not_nominated_by_default() {
    let ledger = Ledger {
        goals_path: PathBuf::new(),
        todo_path: PathBuf::new(),
        goals: vec![Goal {
            body: "Owner billing action required".into(),
            ..goal("Working CI on GitHub", GoalStatus::Blocked)
        }],
        todos: parse_todos(
            "## Working CI on GitHub (blocked — owner action)\n\
                 - [ ] obtain a hosted runner\n",
        ),
    };

    assert_eq!(ledger.open_goals().count(), 1);
    assert_eq!(ledger.open_todo_count(), 1);
    assert_eq!(ledger.actionable_todo_count(), 0);
    assert!(ledger.pick_goal(None).is_none());
    assert!(ledger.next_actionable_todo().is_none());
    assert!(matches!(
        pick_work(&ledger, None, false).expect("default selection"),
        WorkFocus::Stabilize
    ));

    let explicit = pick_work(&ledger, Some("working ci"), false).expect("known explicit goal");
    match explicit {
        WorkFocus::Goal {
            title,
            status,
            todo,
            ..
        } => {
            assert_eq!(title, "Working CI on GitHub");
            assert_eq!(status, GoalStatus::Blocked);
            assert_eq!(todo.as_deref(), Some("obtain a hosted runner"));
        }
        WorkFocus::Stabilize => panic!("explicit blocked goal should be inspectable"),
    }
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
