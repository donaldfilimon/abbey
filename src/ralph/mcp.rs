//! Ralph Loop stdio MCP server and its registration helpers.

use serde_json::Value;
use std::io::{BufRead, Read, Write};

use crate::improve::gate;

use super::*;

/// Register `ralph-loop-rust` in global and workspace MCP configuration files.
pub fn cmd_install_mcp(root: &Path, opts: &RalphOpts) -> Result<i32> {
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("abbey"));
    let bin_str = exe.display().to_string();
    let mut updated_files = Vec::new();

    let mut json_targets = vec![root.join(".mcp.json")];
    if let Some(home) = dirs::home_dir() {
        json_targets.push(home.join(".cursor/mcp.json"));
        json_targets.push(home.join(".claude/mcp.json"));
        json_targets.push(home.join(".config/claude/mcp.json"));
    }

    for path in json_targets {
        if register_json_mcp(&path, &bin_str, &["ralph", "mcp"]).is_ok() {
            updated_files.push(path.display().to_string());
        }
    }

    if let Some(home) = dirs::home_dir() {
        let codex_path = home.join(".codex/config.toml");
        if register_codex_toml_mcp(&codex_path, &bin_str).is_ok() {
            updated_files.push(codex_path.display().to_string());
        }
    }

    if opts.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "server": "ralph-loop-rust",
                "command": bin_str,
                "args": ["ralph", "mcp"],
                "updated_configs": updated_files,
            }))?
        );
    } else {
        println!(
            "abbey ralph install-mcp: registered `ralph-loop-rust` ({bin_str} ralph mcp) in {} config(s):",
            updated_files.len()
        );
        for f in &updated_files {
            println!("  {f}");
        }
    }
    Ok(0)
}

fn register_json_mcp(path: &Path, command: &str, args: &[&str]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut doc: Value = if path.is_file() {
        let raw = fs::read_to_string(path)?;
        serde_json::from_str(&raw).unwrap_or_else(|_| json!({}))
    } else {
        json!({})
    };
    let Some(root_obj) = doc.as_object_mut() else {
        bail!("MCP config root is not a JSON object");
    };
    let servers = root_obj
        .entry("mcpServers")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("mcpServers is not an object"))?;
    servers.insert(
        "ralph-loop-rust".into(),
        json!({
            "command": command,
            "args": args,
        }),
    );
    fs::write(path, serde_json::to_string_pretty(&doc)? + "\n")?;
    Ok(())
}

fn register_codex_toml_mcp(path: &Path, command: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let existing = if path.is_file() {
        fs::read_to_string(path)?
    } else {
        String::new()
    };
    if existing.contains("[mcp_servers.ralph-loop-rust]")
        || existing.contains("[mcp_servers.\"ralph-loop-rust\"]")
    {
        return Ok(());
    }
    let mut updated = existing;
    if !updated.is_empty() && !updated.ends_with('\n') {
        updated.push('\n');
    }
    updated.push_str(&format!(
        "\n[mcp_servers.ralph-loop-rust]\ncommand = \"{command}\"\nargs = [\"ralph\", \"mcp\"]\n"
    ));
    fs::write(path, updated)?;
    Ok(())
}

/// Serve the self-learning Ralph Loop MCP server over newline-delimited JSON-RPC 2.0 stdio.
pub fn serve_mcp_stdio(default_root: &Path, abbey_state: Option<&AbbeyState>) -> Result<i32> {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    serve_mcp_stream(stdin.lock(), stdout.lock(), default_root, abbey_state)?;
    Ok(0)
}

/// Run the Ralph MCP JSON-RPC 2.0 loop over arbitrary buffered streams.
pub fn serve_mcp_stream(
    mut reader: impl BufRead,
    mut writer: impl Write,
    default_root: &Path,
    abbey_state: Option<&AbbeyState>,
) -> std::io::Result<()> {
    let mut initialized = false;
    loop {
        let mut frame = Vec::new();
        let read = (&mut reader)
            .take((MAX_MCP_FRAME_BYTES + 1) as u64)
            .read_until(b'\n', &mut frame)?;
        if read == 0 {
            return Ok(());
        }
        while frame.last().is_some_and(|b| *b == b'\n' || *b == b'\r') {
            frame.pop();
        }
        if frame.is_empty() {
            continue;
        }
        if let Some(resp) = handle_mcp_frame(&frame, &mut initialized, default_root, abbey_state) {
            let line = serde_json::to_string(&resp).unwrap_or_else(|_| {
                r#"{"jsonrpc":"2.0","id":null,"error":{"code":-32603,"message":"serialization error"}}"#
                    .to_string()
            });
            writer.write_all(line.as_bytes())?;
            writer.write_all(b"\n")?;
            writer.flush()?;
        }
    }
}

fn handle_mcp_frame(
    frame: &[u8],
    initialized: &mut bool,
    default_root: &Path,
    abbey_state: Option<&AbbeyState>,
) -> Option<Value> {
    let doc: Value = match serde_json::from_slice(frame) {
        Ok(v) => v,
        Err(e) => {
            return Some(json!({
                "jsonrpc": "2.0",
                "id": Value::Null,
                "error": {"code": -32700, "message": format!("parse error: {e}")}
            }));
        }
    };
    let obj = doc.as_object()?;
    let id = obj.get("id").cloned();
    let method = obj.get("method").and_then(Value::as_str).unwrap_or("");
    let params = obj.get("params").cloned().unwrap_or_else(|| json!({}));

    let id = id?;
    let result = match method {
        "initialize" => {
            *initialized = true;
            let version = params
                .get("protocolVersion")
                .and_then(Value::as_str)
                .unwrap_or("2025-06-18");
            Ok(json!({
                "protocolVersion": version,
                "capabilities": {"tools": {"listChanged": false}},
                "serverInfo": {
                    "name": "ralph-loop-rust",
                    "title": "Abbey Self-Learning Rust Ralph Loop MCP",
                    "version": crate::build_info::VERSION
                },
                "instructions": "Self-learning Rust Ralph loop orchestrator. Provides fresh-context single-task planning, playbook learning, failure reflection logging, and circuit-breaker state."
            }))
        }
        "ping" => Ok(json!({})),
        "tools/list" => {
            if !*initialized {
                Err((-32600, "server not initialized".to_string()))
            } else {
                Ok(json!({"tools": ralph_mcp_tools()}))
            }
        }
        "tools/call" => {
            if !*initialized {
                Err((-32600, "server not initialized".to_string()))
            } else {
                call_ralph_mcp_tool(&params, default_root, abbey_state)
            }
        }
        other => Err((-32601, format!("unknown method `{other}`"))),
    };

    Some(match result {
        Ok(val) => json!({"jsonrpc": "2.0", "id": id, "result": val}),
        Err((code, msg)) => json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": {"code": code, "message": msg}
        }),
    })
}

fn ralph_mcp_tools() -> Vec<Value> {
    vec![
        json!({
            "name": "ralph_status",
            "title": "Ralph Loop Status",
            "description": "Inspect the Rust Ralph loop workspace status: PROMPT.md, fix_plan.md pending/completed tasks, next single task, self-learning count, and circuit-breaker state.",
            "inputSchema": {
                "$schema": "https://json-schema.org/draft/2020-12/schema",
                "type": "object",
                "properties": {
                    "workspace": {"type": "string", "description": "Optional workspace path"}
                },
                "additionalProperties": false
            }
        }),
        json!({
            "name": "ralph_init",
            "title": "Initialize Ralph Loop Harness",
            "description": "Scaffold PROMPT.md, fix_plan.md, .ralph/learnings.md, .ralph/errors.log, and .ralph/state.json for a Rust repository.",
            "inputSchema": {
                "$schema": "https://json-schema.org/draft/2020-12/schema",
                "type": "object",
                "properties": {
                    "workspace": {"type": "string"},
                    "force": {"type": "boolean"}
                },
                "additionalProperties": false
            }
        }),
        json!({
            "name": "ralph_plan",
            "title": "Build Fresh-Context Single-Task Prompt",
            "description": "Select the single highest-priority pending task from fix_plan.md and assemble a fresh-context iteration prompt with self-learned rules and failure reflections.",
            "inputSchema": {
                "$schema": "https://json-schema.org/draft/2020-12/schema",
                "type": "object",
                "properties": {
                    "workspace": {"type": "string"},
                    "hint": {"type": "string"}
                },
                "additionalProperties": false
            }
        }),
        json!({
            "name": "ralph_learn",
            "title": "Record Self-Learned Rust Insight",
            "description": "Persist a reusable Rust compiler, borrow-checker, or architecture insight into .ralph/learnings.md and Abbey LTM.",
            "inputSchema": {
                "$schema": "https://json-schema.org/draft/2020-12/schema",
                "type": "object",
                "properties": {
                    "workspace": {"type": "string"},
                    "insight": {"type": "string", "minLength": 1}
                },
                "required": ["insight"],
                "additionalProperties": false
            }
        }),
        json!({
            "name": "ralph_record_iteration",
            "title": "Record Iteration Verification Outcome",
            "description": "Record the outcome of a single-task Ralph iteration (gate pass or failure excerpt), updating fix_plan.md, .ralph/errors.log, .ralph/learnings.md, and circuit-breaker state.",
            "inputSchema": {
                "$schema": "https://json-schema.org/draft/2020-12/schema",
                "type": "object",
                "properties": {
                    "workspace": {"type": "string"},
                    "task": {"type": "string"},
                    "gate_ok": {"type": "boolean"},
                    "excerpt": {"type": "string"},
                    "max_failures": {"type": "integer", "minimum": 1}
                },
                "required": ["task", "gate_ok"],
                "additionalProperties": false
            }
        }),
    ]
}

fn call_ralph_mcp_tool(
    params: &Value,
    default_root: &Path,
    abbey_state: Option<&AbbeyState>,
) -> std::result::Result<Value, (i64, String)> {
    let Some(name) = params.get("name").and_then(Value::as_str) else {
        return Err((-32602, "`name` is required".into()));
    };
    let args = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let root = args
        .get("workspace")
        .and_then(Value::as_str)
        .map(PathBuf::from)
        .unwrap_or_else(|| default_root.to_path_buf());

    let outcome: Result<Value> = match name {
        "ralph_status" => Ok(
            serde_json::to_value(status_snapshot(&root)).map_err(|e| (-32603, e.to_string()))?
        ),
        "ralph_init" => {
            let force = args.get("force").and_then(Value::as_bool).unwrap_or(false);
            let files = scaffold_workspace(&root, force).map_err(|e| (-32603, e.to_string()))?;
            Ok(json!({
                "workspace": root.display().to_string(),
                "written": files.iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
            }))
        }
        "ralph_plan" => {
            let hint = args.get("hint").and_then(Value::as_str);
            let (task, prompt) = build_iteration_prompt(&root, abbey_state, hint)
                .map_err(|e| (-32603, e.to_string()))?;
            Ok(json!({
                "selected_task": task,
                "prompt": prompt,
            }))
        }
        "ralph_learn" => {
            let insight = args.get("insight").and_then(Value::as_str).unwrap_or("");
            let ltm_id = record_learning(&root, abbey_state, insight)
                .map_err(|e| (-32603, e.to_string()))?;
            Ok(json!({
                "recorded": insight,
                "ltm_id": ltm_id,
            }))
        }
        "ralph_record_iteration" => {
            let task = args
                .get("task")
                .and_then(Value::as_str)
                .unwrap_or("unspecified task");
            let gate_ok = args
                .get("gate_ok")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let excerpt = args.get("excerpt").and_then(Value::as_str).unwrap_or("");
            let max_failures = args
                .get("max_failures")
                .and_then(Value::as_u64)
                .map(|n| n as usize)
                .unwrap_or(DEFAULT_MAX_FAILURES);
            let mut st = load_state(&root);
            st.iteration += 1;
            if st.active_task.as_deref() != Some(task) {
                st.active_task = Some(task.to_string());
                st.consecutive_failures = 0;
            }
            if gate_ok {
                let _ = mark_task_completed(&root, task);
                st.completed_tasks += 1;
                st.consecutive_failures = 0;
                st.last_gate_ok = Some(true);
                st.last_gate_kinds = "ok".into();
                st.halted_reason = None;
                let _ = record_learning(
                    &root,
                    abbey_state,
                    &format!("Iteration {} completed `{task}` cleanly.", st.iteration),
                );
            } else {
                st.consecutive_failures += 1;
                st.last_gate_ok = Some(false);
                let kinds = gate::classify_failures(excerpt)
                    .iter()
                    .map(|k| k.label())
                    .collect::<Vec<_>>()
                    .join(",");
                let kinds_csv = if kinds.is_empty() {
                    "other".to_string()
                } else {
                    kinds
                };
                st.last_gate_kinds = kinds_csv.clone();
                record_failure_reflection(
                    &root,
                    abbey_state,
                    st.iteration,
                    task,
                    &kinds_csv,
                    excerpt,
                )
                .map_err(|e| (-32603, e.to_string()))?;
                if st.consecutive_failures >= max_failures {
                    st.halted_reason = Some(format!(
                        "task `{task}` failed {} consecutive iterations",
                        st.consecutive_failures
                    ));
                }
            }
            save_state(&root, &st).map_err(|e| (-32603, e.to_string()))?;
            Ok(serde_json::to_value(&st).map_err(|e| (-32603, e.to_string()))?)
        }
        other => return Err((-32602, format!("unknown tool `{other}`"))),
    };

    match outcome {
        Ok(structured) => {
            let text = serde_json::to_string_pretty(&structured)
                .unwrap_or_else(|_| structured.to_string());
            Ok(json!({
                "content": [{"type": "text", "text": text}],
                "structuredContent": structured,
                "isError": false
            }))
        }
        Err(err) => Ok(json!({
            "content": [{"type": "text", "text": err.to_string()}],
            "isError": true
        })),
    }
}
