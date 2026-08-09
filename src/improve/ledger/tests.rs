use super::*;

fn goal_text(id: &str) -> String {
    format!(
        "## Strict ID\n\
         <!-- abbey-goal\n\
         id: {id}\n\
         status: todo\n\
         implementation-evidence: none\n\
         automated-test-evidence: none\n\
         live-external-evidence: none\n\
         next-action: implement strict parsing\n\
         -->\n"
    )
}

#[test]
fn goal_ids_require_lowercase_ascii_kebab_case() {
    for valid in ["a", "goal-2", "phase-3-claims-ledger"] {
        let goals = parse_goals(&goal_text(valid)).expect("valid kebab-case ID");
        assert_eq!(goals[0].id, valid);
    }

    for invalid in [
        "../not-kebab",
        "Uppercase",
        "under_score",
        "double--hyphen",
        "-leading",
        "trailing-",
    ] {
        let error = parse_goals(&goal_text(invalid)).expect_err("invalid ID must fail closed");
        assert!(
            error
                .to_string()
                .contains("expected lowercase ASCII kebab-case"),
            "unexpected error for {invalid}: {error}"
        );
    }
}

#[test]
fn unknown_explicit_hint_errors_instead_of_selecting_default_goal() {
    let ledger = Ledger {
        goals_path: PathBuf::new(),
        todo_path: PathBuf::new(),
        goals: parse_goals(&goal_text("known-goal")).expect("valid fixture"),
        todos: Vec::new(),
    };

    assert!(ledger.pick_goal(Some("does-not-exist")).is_none());
    let error = pick_work(&ledger, Some("does-not-exist"), false)
        .expect_err("unknown explicit hints must fail closed");
    assert!(
        error
            .to_string()
            .contains("unknown goal hint `does-not-exist`")
    );

    assert!(matches!(
        pick_work(&ledger, Some("  "), false).expect("empty hint preserves default"),
        WorkFocus::Goal { .. }
    ));
    assert!(matches!(
        pick_work(&ledger, None, false).expect("no hint preserves default"),
        WorkFocus::Goal { .. }
    ));
}
