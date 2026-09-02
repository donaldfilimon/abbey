use super::*;
use std::path::PathBuf;

#[test]
fn flag_shaped_prompts_are_detected() {
    // Only the first non-empty word matters — that is the one the backend
    // sees in option position.
    assert!(looks_like_flags(&["--force".to_string()]));
    assert!(looks_like_flags(&["  ".to_string(), "-p".to_string()]));
    assert!(!looks_like_flags(&["explain --force".to_string()]));
    assert!(!looks_like_flags(&[]));
}

#[test]
fn abbey_generated_prompts_are_not_flagged() {
    // please-fix and the hybrid loop wrap untrusted text mid-string, so the
    // argv element begins with prose even when the text itself has flags.
    assert!(!looks_like_flags(&[
        "Please fix this failure:\n\n--force --yolo".to_string()
    ]));
    assert!(!looks_like_flags(&[
        "Stage 2 of 2 (implement).\n--- interpretation ---\n--force".to_string()
    ]));
}

/// A config with every cursor-agent knob turned on — the `fm` builder must
/// still emit only flags `fm respond` actually accepts.
fn maximal_cursor_config() -> AgentConfig {
    AgentConfig {
        agent_path: PathBuf::from("/usr/bin/fm"),
        model: "claude-fable-5-thinking-high".into(),
        auto_review: true,
        trust: true,
        force: true,
        no_resume: true,
        mode: Some("plan".into()),
        print: true,
        output_format: Some("json".into()),
        worktree: Some(Worktree::Named("wt".into())),
        workspace: Some(PathBuf::from("/ws")),
        add_dirs: vec![PathBuf::from("/extra")],
        sandbox: Some("enabled".into()),
        extra_args: vec!["--debug".into(), "--max-turns".into(), "7".into()],
        backend: AgentBackend::Fm,
        transcript_dir: None,
        media_note: None,
        media_prefers_gemma: false,
        force_capture: false,
        cot_path: None,
    }
}

#[test]
fn fm_argv_never_leaks_cursor_flags() {
    let argv = maximal_cursor_config().build_args(None, &["hello".into()]);
    // Every flag `fm respond` knows about.
    let allowed = [
        "respond",
        "--model",
        "system",
        "pcc",
        "--instructions",
        "--no-stream",
        "--resume",
        "--save-transcript",
        "hello",
    ];
    for a in &argv {
        if a.starts_with("--") {
            assert!(
                allowed.contains(&a.as_str()),
                "leaked non-fm flag {a:?} into argv {argv:?}"
            );
        }
    }
    for banned in [
        "--auto-review",
        "--trust",
        "--force",
        "--mode",
        "--worktree",
        "--workspace",
        "--add-dir",
        "--sandbox",
        "--output-format",
        "--print",
        "--debug",
        "--max-turns",
    ] {
        assert!(!argv.contains(&banned.to_string()), "{banned} leaked");
    }
}

#[test]
fn fm_collapses_any_cursor_model_id_to_system() {
    assert_eq!(fm_model("claude-fable-5-thinking-high"), "system");
    assert_eq!(fm_model("composer-2.5"), "system");
    assert_eq!(fm_model("auto"), "system");
    assert_eq!(fm_model("pcc"), "pcc");
    assert_eq!(fm_model("private-cloud-compute"), "pcc");
    assert_eq!(fm_model("private_cloud_compute"), "pcc");
    // Substrings must not hijack unrelated aliases.
    assert_eq!(fm_model("Private Cloud Compute"), "system");
    assert_eq!(fm_model("my-cloud-model"), "system");
}

#[test]
fn fm_mode_becomes_instructions() {
    let mut cfg = maximal_cursor_config();
    cfg.mode = Some("ask".into());
    let argv = cfg.build_args(None, &["q".into()]);
    let i = argv.iter().position(|a| a == "--instructions").unwrap();
    assert!(argv[i + 1].contains("Do not modify files"));
}

#[test]
fn fm_resume_maps_a_chat_id_onto_a_transcript_file() {
    let dir = std::env::temp_dir().join(format!("abbey-fm-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut cfg = maximal_cursor_config();
    cfg.transcript_dir = Some(dir.clone());

    // No transcript yet: save but do not resume from a file that isn't there.
    let argv = cfg.build_args(Some("chat-1"), &["hi".into()]);
    assert!(!argv.contains(&"--resume".to_string()));
    let save = argv.iter().position(|a| a == "--save-transcript").unwrap();
    assert!(argv[save + 1].ends_with("chat-1.transcript"));

    // Once it exists, resume from it.
    std::fs::write(dir.join("chat-1.transcript"), "{}").unwrap();
    let argv = cfg.build_args(Some("chat-1"), &["hi".into()]);
    let r = argv.iter().position(|a| a == "--resume").unwrap();
    assert!(argv[r + 1].ends_with("chat-1.transcript"));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn abi_argv_never_leaks_cursor_or_fm_flags() {
    let mut cfg = maximal_cursor_config();
    cfg.backend = AgentBackend::Abi;
    // resume_id must be ignored: `abi complete` has no resume surface.
    let argv = cfg.build_args(Some("chat-1"), &["hello".into()]);
    let dashdash = argv.iter().position(|a| a == "--").expect("-- separator");
    for a in &argv[..dashdash] {
        assert!(
            ["complete", "--live", "--model"].contains(&a.as_str()) || !a.starts_with("--"),
            "leaked non-abi flag {a:?} into argv {argv:?}"
        );
    }
    for banned in [
        "--auto-review",
        "--trust",
        "--force",
        "--mode",
        "--worktree",
        "--workspace",
        "--add-dir",
        "--sandbox",
        "--output-format",
        "--print",
        "--debug",
        "--max-turns",
        "--instructions",
        "--no-stream",
        "--save-transcript",
        "--resume",
    ] {
        assert!(!argv.contains(&banned.to_string()), "{banned} leaked");
    }
    assert!(!argv.contains(&"chat-1".to_string()), "resume id leaked");
}

#[test]
fn abi_prompt_always_follows_a_double_dash() {
    let mut cfg = maximal_cursor_config();
    cfg.backend = AgentBackend::Abi;
    cfg.mode = None;
    cfg.model = "auto".into();
    // A flag-shaped prompt must land after `--`, i.e. as text.
    let argv = cfg.build_args(None, &["--force".into()]);
    let dashdash = argv.iter().position(|a| a == "--").expect("-- separator");
    let prompt = argv.iter().position(|a| a == "--force").expect("prompt");
    assert!(prompt > dashdash, "prompt reached abi in option position");
}

#[test]
fn abi_transport_is_live_only_for_explicit_aliases() {
    assert_eq!(abi_transport("auto"), AbiTransport::Local);
    assert_eq!(abi_transport("fable"), AbiTransport::Local);
    assert_eq!(abi_transport("composer-2.5"), AbiTransport::Local);
    // Cursor leftovers / Max bindings must not silently select --live.
    assert_eq!(
        abi_transport("claude-fable-5-thinking-high"),
        AbiTransport::Local
    );
    assert_eq!(abi_normalize_model("claude-fable-5-thinking-high"), "local");
    assert_eq!(abi_normalize_model("fable"), "local");
    // Substrings must not hijack unrelated ids into a network call.
    assert_eq!(abi_transport("my-live-model"), AbiTransport::Local);
    assert_eq!(abi_transport("anthropic-ish"), AbiTransport::Local);
    assert_eq!(abi_transport("live"), AbiTransport::Live(None));
    assert_eq!(abi_transport("anthropic"), AbiTransport::Live(None));
    assert_eq!(
        abi_transport("claude-fable-5"),
        AbiTransport::Live(Some("claude-fable-5".into()))
    );
}

#[test]
fn abi_local_argv_uses_normalized_model_not_cursor_binding() {
    let mut cfg = maximal_cursor_config();
    cfg.backend = AgentBackend::Abi;
    cfg.mode = None;
    cfg.model = "claude-fable-5-thinking-high".into();
    let argv = cfg.build_args(None, &["hi".into()]);
    assert!(
        !argv.contains(&"--live".to_string()),
        "silent live: {argv:?}"
    );
    let model_i = argv.iter().position(|a| a == "--model").unwrap();
    assert_eq!(argv[model_i + 1], "local");
}

#[test]
fn abi_resume_carries_bounded_transcript_context() {
    let dir = std::env::temp_dir().join(format!("abbey-abi-ctx-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut cfg = maximal_cursor_config();
    cfg.backend = AgentBackend::Abi;
    cfg.mode = None;
    cfg.model = "auto".into();
    cfg.transcript_dir = Some(dir.clone());

    // No transcript yet → context-free turn, prompt still after `--`.
    let argv = cfg.build_args(Some("chat-9"), &["next".into()]);
    assert!(!argv.iter().any(|a| a.contains("Previous conversation")));

    // With a transcript, the tail rides in as a context element before
    // the prompt — and stays bounded for oversized histories.
    std::fs::write(
        dir.join("chat-9.transcript"),
        format!(
            "### user\nremember the word xyzzy\n### abbey\nnoted\n{}",
            "pad ".repeat(4000)
        ),
    )
    .unwrap();
    let argv = cfg.build_args(Some("chat-9"), &["next".into()]);
    let ctx = argv
        .iter()
        .find(|a| a.contains("Previous conversation"))
        .expect("context element");
    assert!(
        ctx.len() <= ABI_CONTEXT_TAIL_BYTES + 200,
        "context unbounded: {}",
        ctx.len()
    );
    let dashdash = argv.iter().position(|a| a == "--").unwrap();
    let ctx_pos = argv
        .iter()
        .position(|a| a.contains("Previous conversation"))
        .unwrap();
    assert!(
        ctx_pos > dashdash,
        "context must be input text, not options"
    );
    assert_eq!(argv.last().unwrap(), "next");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn utf8_tail_respects_char_boundaries() {
    let s = "é".repeat(100); // 2 bytes each
    let tail = utf8_tail(&s, 5);
    assert!(tail.len() <= 5);
    assert!(tail.chars().all(|c| c == 'é'));
    assert_eq!(utf8_tail("short", 100), "short");
}

#[test]
fn abi_mode_rides_in_the_input_text() {
    let mut cfg = maximal_cursor_config();
    cfg.backend = AgentBackend::Abi;
    cfg.mode = Some("ask".into());
    let argv = cfg.build_args(None, &["q".into()]);
    let dashdash = argv.iter().position(|a| a == "--").unwrap();
    // No --instructions flag exists on abi; the note is input text.
    assert!(argv[dashdash + 1].contains("Do not modify files"));
    assert_eq!(argv.last().unwrap(), "q");
}

#[test]
fn claude_argv_never_leaks_cursor_or_fm_flags() {
    let mut cfg = maximal_cursor_config();
    cfg.backend = AgentBackend::Claude;
    let argv = cfg.build_args(None, &["hello".into()]);
    let allowed = [
        "--model",
        "--print",
        "--output-format",
        "--permission-mode",
        "--append-system-prompt",
        "--add-dir",
        "--resume",
        "--session-id",
    ];
    for a in &argv {
        if a.starts_with("--") {
            assert!(
                allowed.contains(&a.as_str()),
                "leaked non-claude flag {a:?} into argv {argv:?}"
            );
        }
    }
    for banned in [
        "--auto-review",
        "--trust",
        "--force",
        "--mode",
        "--worktree",
        "--workspace",
        "--sandbox",
        "--debug",
        "--max-turns",
        "--instructions",
        "--no-stream",
        "--save-transcript",
        "--live",
    ] {
        assert!(!argv.contains(&banned.to_string()), "{banned} leaked");
    }
    // Cursor thinking binding collapses to a real catalog id.
    let m = argv.iter().position(|a| a == "--model").unwrap();
    assert_eq!(argv[m + 1], "claude-fable-5");
    // --force (always-approve) becomes claude's bypassPermissions and wins
    // over plan mode — exactly one permission mode is emitted.
    let pm = argv.iter().position(|a| a == "--permission-mode").unwrap();
    assert_eq!(argv[pm + 1], "bypassPermissions");
    assert_eq!(argv.iter().filter(|a| *a == "--permission-mode").count(), 1);
    assert_eq!(argv.last().unwrap(), "hello");
}

#[test]
fn claude_model_vocabulary_clamp() {
    // Abbey's flagship default and role aliases.
    assert_eq!(claude_model("opus").as_deref(), Some("opus"));
    assert_eq!(claude_model("max").as_deref(), Some("opus"));
    assert_eq!(claude_model("gemma").as_deref(), Some("sonnet"));
    assert_eq!(claude_model("composer-2.5").as_deref(), Some("sonnet"));
    // auto must stay claude's own plan default — plans that reject named
    // models only work when --model is omitted entirely.
    assert_eq!(claude_model("auto"), None);
    assert_eq!(claude_model(""), None);
    // Cursor bindings collapse to catalog ids; bare ids pass through.
    assert_eq!(
        claude_model("claude-opus-5-thinking-high").as_deref(),
        Some("claude-opus-5")
    );
    assert_eq!(
        claude_model("claude-opus-5").as_deref(),
        Some("claude-opus-5")
    );
    // Foreign executors' ids clamp to the flagship default.
    assert_eq!(claude_model("gpt-5.6-sol-high").as_deref(), Some("opus"));
    assert_eq!(
        claude_model("cursor-grok-4.5-high").as_deref(),
        Some("opus")
    );
    assert_eq!(claude_model("kimi-k3-high").as_deref(), Some("opus"));
}

#[test]
fn claude_session_mints_then_resumes() {
    let dir = std::env::temp_dir().join(format!("abbey-claude-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut cfg = maximal_cursor_config();
    cfg.backend = AgentBackend::Claude;
    cfg.force = false;
    cfg.mode = None;
    cfg.transcript_dir = Some(dir.clone());

    // No marker yet: the id is minted as a fresh claude session.
    let argv = cfg.build_args(Some("chat-7"), &["hi".into()]);
    let s = argv.iter().position(|a| a == "--session-id").unwrap();
    assert_eq!(argv[s + 1], "chat-7");
    assert!(!argv.contains(&"--resume".to_string()));

    // After a successful run recorded the marker, later turns resume.
    cfg.touch_claude_session_marker("chat-7");
    let argv = cfg.build_args(Some("chat-7"), &["hi".into()]);
    let r = argv.iter().position(|a| a == "--resume").unwrap();
    assert_eq!(argv[r + 1], "chat-7");
    assert!(!argv.contains(&"--session-id".to_string()));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn claude_plan_mode_maps_to_permission_mode() {
    let mut cfg = maximal_cursor_config();
    cfg.backend = AgentBackend::Claude;
    cfg.force = false;
    cfg.mode = Some("plan".into());
    let argv = cfg.build_args(None, &["q".into()]);
    let pm = argv.iter().position(|a| a == "--permission-mode").unwrap();
    assert_eq!(argv[pm + 1], "plan");
    let sp = argv
        .iter()
        .position(|a| a == "--append-system-prompt")
        .unwrap();
    assert!(argv[sp + 1].contains("plan only"));
}

#[test]
fn ollama_argv_never_leaks_cursor_flags() {
    let mut cfg = maximal_cursor_config();
    cfg.backend = AgentBackend::Ollama;
    cfg.model = "auto".into();
    let argv = cfg.build_args(Some("chat-1"), &["hello".into()]);
    let dashdash = argv.iter().position(|a| a == "--").expect("-- separator");
    for a in &argv[..dashdash] {
        assert!(
            ["run", "--nowordwrap"].contains(&a.as_str()) || !a.starts_with("--"),
            "leaked non-ollama flag {a:?} into argv {argv:?}"
        );
    }
    for banned in [
        "--auto-review",
        "--trust",
        "--force",
        "--mode",
        "--worktree",
        "--workspace",
        "--add-dir",
        "--sandbox",
        "--output-format",
        "--print",
        "--debug",
        "--max-turns",
        "--instructions",
        "--no-stream",
        "--save-transcript",
        "--resume",
        "--live",
        "--model",
    ] {
        assert!(!argv.contains(&banned.to_string()), "{banned} leaked");
    }
    assert!(!argv.contains(&"chat-1".to_string()), "resume id leaked");
    assert_eq!(argv[0], "run");
    assert_eq!(argv[2], crate::models::OLLAMA_DEFAULT_MODEL);
}

#[test]
fn ollama_aliases_collapse_to_local_gemma4() {
    for alias in [
        "auto",
        "gemma",
        "gemma:27b-mlx",
        "gemma4:26b-mlx",
        "claude-fable-5-thinking-high",
        "composer-2.5",
        "opus",
    ] {
        assert_eq!(
            ollama_normalize_model(alias),
            crate::models::OLLAMA_DEFAULT_MODEL,
            "{alias}"
        );
    }
    assert_eq!(ollama_normalize_model("gemma4:12b-mlx"), "gemma4:12b-mlx");
}

#[test]
fn ollama_prompt_always_follows_a_double_dash() {
    let mut cfg = maximal_cursor_config();
    cfg.backend = AgentBackend::Ollama;
    cfg.mode = None;
    cfg.model = "auto".into();
    let argv = cfg.build_args(None, &["--force".into()]);
    let dashdash = argv.iter().position(|a| a == "--").expect("-- separator");
    let prompt = argv.iter().position(|a| a == "--force").expect("prompt");
    assert!(
        prompt > dashdash,
        "prompt reached ollama in option position"
    );
}

#[test]
fn ollama_resume_carries_bounded_transcript_context() {
    let dir = std::env::temp_dir().join(format!("abbey-ollama-ctx-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut cfg = maximal_cursor_config();
    cfg.backend = AgentBackend::Ollama;
    cfg.mode = None;
    cfg.model = "auto".into();
    cfg.transcript_dir = Some(dir.clone());

    let argv = cfg.build_args(Some("chat-9"), &["next".into()]);
    assert!(!argv.iter().any(|a| a.contains("Previous conversation")));

    std::fs::write(
        dir.join("chat-9.transcript"),
        format!(
            "### user\nremember the word xyzzy\n### abbey\nnoted\n{}",
            "pad ".repeat(4000)
        ),
    )
    .unwrap();
    let argv = cfg.build_args(Some("chat-9"), &["next".into()]);
    let ctx = argv
        .iter()
        .find(|a| a.contains("Previous conversation"))
        .expect("context element");
    assert!(
        ctx.len() <= ABI_CONTEXT_TAIL_BYTES + 200,
        "context unbounded: {}",
        ctx.len()
    );
    let dashdash = argv.iter().position(|a| a == "--").unwrap();
    let ctx_pos = argv
        .iter()
        .position(|a| a.contains("Previous conversation"))
        .unwrap();
    assert!(
        ctx_pos > dashdash,
        "context must be input text, not options"
    );
    assert_eq!(argv.last().unwrap(), "next");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn cursor_backend_argv_is_unchanged_by_the_fm_split() {
    let mut cfg = maximal_cursor_config();
    cfg.backend = AgentBackend::Cursor;
    let argv = cfg.build_args(Some("abc"), &["hi".into()]);
    for expected in [
        "--auto-review",
        "--trust",
        "--force",
        "--sandbox",
        "--resume",
    ] {
        assert!(argv.contains(&expected.to_string()), "{expected} missing");
    }
}

#[test]
fn truncate_utf8_bytes_respects_char_boundaries_and_cap() {
    let cap = max_prompt_argv_bytes();
    let s = "a".repeat(cap + 50_000);
    let out = truncate_utf8_bytes(&s, cap);
    assert!(out.len() <= cap);
    assert!(out.contains("truncated for OS argv limit"));
}

#[test]
fn clamp_prompt_args_caps_each_trailing_string() {
    let cap = max_prompt_argv_bytes();
    let huge = "x".repeat(cap + 10_000);
    let argv = clamp_prompt_args(&[huge, "ok".into()]);
    assert!(argv[0].len() <= cap);
    assert_eq!(argv[1], "ok");
}

#[test]
fn build_args_clamps_a_please_fix_sized_prompt() {
    let mut cfg = maximal_cursor_config();
    cfg.backend = AgentBackend::Cursor;
    let cap = max_prompt_argv_bytes();
    let huge = "y".repeat(cap + 80_000);
    let argv = cfg.build_args(None, &[huge]);
    let last = argv.last().expect("prompt");
    assert!(
        last.len() <= cap,
        "prompt argv still too long: {}",
        last.len()
    );
}
