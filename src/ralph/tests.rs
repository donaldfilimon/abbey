use super::*;
use serde_json::Value;

fn scratch_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "abbey-ralph-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn scaffold_and_single_task_completion_round_trip() {
    let dir = scratch_dir("scaffold");
    let files = scaffold_workspace(&dir, false).unwrap();
    assert_eq!(files.len(), 5);

    let snap0 = status_snapshot(&dir);
    assert!(snap0.prompt_exists);
    assert!(snap0.fix_plan_exists);
    assert_eq!(snap0.pending_count, 2);
    assert_eq!(snap0.completed_count, 0);

    let first_task = snap0.next_task.unwrap();
    assert!(mark_task_completed(&dir, &first_task).unwrap());

    let snap1 = status_snapshot(&dir);
    assert_eq!(snap1.pending_count, 1);
    assert_eq!(snap1.completed_count, 1);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn failure_reflection_auto_learns_and_trips_circuit_breaker() {
    let dir = scratch_dir("breaker");
    scaffold_workspace(&dir, false).unwrap();

    let mut input = Vec::new();
    let reqs = [
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}"#,
            &serde_json::to_string(&json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "tools/call",
                "params": {
                    "name": "ralph_record_iteration",
                    "arguments": {
                        "workspace": dir.display().to_string(),
                        "task": "Fix borrow checker lifetime in parser",
                        "gate_ok": false,
                        "excerpt": "error[E0502]: cannot borrow `buf` as mutable because it is also borrowed as immutable",
                        "max_failures": 2
                    }
                }
            }))
            .unwrap(),
            &serde_json::to_string(&json!({
                "jsonrpc": "2.0",
                "id": 3,
                "method": "tools/call",
                "params": {
                    "name": "ralph_record_iteration",
                    "arguments": {
                        "workspace": dir.display().to_string(),
                        "task": "Fix borrow checker lifetime in parser",
                        "gate_ok": false,
                        "excerpt": "error[E0502]: cannot borrow `buf` as mutable because it is also borrowed as immutable",
                        "max_failures": 2
                    }
                }
            }))
            .unwrap(),
        ];
    for r in reqs {
        input.extend_from_slice(r.as_bytes());
        input.push(b'\n');
    }
    let mut output = Vec::new();
    serve_mcp_stream(&input[..], &mut output, &dir, None).unwrap();
    let responses: Vec<Value> = String::from_utf8(output)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(responses.len(), 3);
    let st = load_state(&dir);
    assert_eq!(st.consecutive_failures, 2);
    assert!(st.halted_reason.is_some());
    let learnings = fs::read_to_string(learnings_path(&dir)).unwrap();
    assert!(learnings.contains("E0502"));
    let _ = fs::remove_dir_all(&dir);
}
