//! Streaming mode of [`super::run_once`]: same argv, same continuity, but the
//! child's stdout is decoded live into [`StreamEvent`]s on the caller's tap.

use super::{AgentBackend, AgentConfig, MAX_CAPTURE_BYTES};
use crate::stream::{StreamEvent, StreamTap};
use anyhow::Result;
use std::sync::mpsc;

pub(super) const CANCELLED_EXIT: i32 = 130;

enum Ended {
    Exited {
        code: i32,
        success: bool,
        stderr: String,
    },
    Cancelled,
}

pub(super) fn run_once_streaming(
    cfg: &AgentConfig,
    tap: &StreamTap,
    resume_id: Option<&str>,
    prompt_and_rest: &[String],
) -> Result<i32> {
    anyhow::ensure!(
        tap.raw_remaining() > 0,
        "stream exceeded the 4 MiB cumulative raw-output limit"
    );
    let _turn_lock = if cfg.backend.is_oneshot_local() {
        resume_id
            .filter(|id| !id.is_empty())
            .map(|id| cfg.lock_local_turn(id))
            .transpose()?
            .flatten()
    } else {
        None
    };
    let mut run_cfg = cfg.clone();
    run_cfg.print = true;
    run_cfg.output_format = crate::stream::stream_output_format(cfg.backend).map(str::to_string);

    let (chunk_tx, chunk_rx) = mpsc::channel::<Vec<u8>>();
    let sink = tap.clone();
    let mut decoder = crate::stream::decoder_for(cfg.backend);
    let forwarder = std::thread::Builder::new()
        .name("abbey-stream-decode".into())
        .spawn(move || {
            let mut text = String::new();
            let mut send = |ev: StreamEvent| {
                if sink.emit(ev.clone())
                    && let StreamEvent::TextDelta(t) = ev
                {
                    text.push_str(&t);
                }
            };
            for chunk in chunk_rx {
                if !sink.record_raw(chunk.len()) {
                    break;
                }
                decoder.feed(&chunk).into_iter().for_each(&mut send);
            }
            decoder.finish().into_iter().for_each(&mut send);
            let unknown = decoder.unknown_events();
            (text, unknown)
        })?;

    let ended = capture(&run_cfg, resume_id, prompt_and_rest, chunk_tx, tap);
    let (text, unknown) = forwarder
        .join()
        .map_err(|_| anyhow::anyhow!("stream decoder thread panicked"))?;
    if unknown > 0 {
        tap.notice(format!(
            "abbey: {unknown} unrecognised stream event(s) ignored"
        ));
    }
    if tap.exceeded() {
        anyhow::bail!("stream exceeded the 4 MiB decoded-payload or 65,536-event limit");
    }
    if tap.failed() {
        anyhow::bail!("executor reported a failed stream event");
    }
    let ended = match ended {
        Ok(ended) => ended,
        Err(e) => {
            let _ = tap.emit(StreamEvent::Failed(format!("{e:#}")));
            return Err(e);
        }
    };
    let code = match ended {
        Ended::Cancelled => {
            tap.notice("interrupted");
            CANCELLED_EXIT
        }
        Ended::Exited {
            code,
            success,
            stderr,
        } => {
            let stderr = stderr.trim();
            if !stderr.is_empty() {
                tap.notice(stderr.to_string());
            }
            // stderr diagnostics consume the same decoded budget. Never
            // persist a successful turn whose final notice overflowed it.
            if tap.exceeded() {
                anyhow::bail!("stream exceeded the 4 MiB decoded-payload or 65,536-event limit");
            }
            if success
                && !tap.cancel.is_cancelled()
                && let Some(id) = resume_id.filter(|i| !i.is_empty())
            {
                if cfg.backend.is_oneshot_local() {
                    cfg.append_local_transcript(id, prompt_and_rest, &text);
                }
                if cfg.backend == AgentBackend::Claude {
                    cfg.touch_claude_session_marker(id);
                }
            }
            code
        }
    };
    Ok(code)
}

#[cfg(unix)]
fn capture(
    cfg: &AgentConfig,
    resume_id: Option<&str>,
    prompt_and_rest: &[String],
    chunks: mpsc::Sender<Vec<u8>>,
    tap: &StreamTap,
) -> Result<Ended> {
    use crate::runtime::supervisor::{
        ProcessSpec, SupervisorLimits, SupervisorOutcome, run_tapped,
    };
    use std::time::Duration;
    let agent = cfg.exec_path()?;
    let args = cfg.build_args(resume_id, prompt_and_rest);
    let spec = ProcessSpec::inherited(
        agent.clone(),
        args.iter().map(std::ffi::OsString::from).collect(),
    );
    let limits = SupervisorLimits {
        timeout: Duration::from_secs(30 * 60),
        terminate_grace: Duration::from_secs(1),
        stdout_bytes: tap.raw_remaining(),
        stderr_bytes: MAX_CAPTURE_BYTES,
        poll_interval: Duration::from_millis(20),
    };
    let cancel = tap.cancel.clone();
    match run_tapped(&spec, &limits, move || cancel.is_cancelled(), chunks) {
        Ok(SupervisorOutcome::Exited { status, stderr, .. }) => Ok(Ended::Exited {
            code: status.code().unwrap_or(1),
            success: status.success(),
            stderr: String::from_utf8_lossy(&stderr).into_owned(),
        }),
        Ok(SupervisorOutcome::Cancelled) => Ok(Ended::Cancelled),
        Ok(SupervisorOutcome::TimedOut) => {
            anyhow::bail!("agent run exceeded the 30-minute limit")
        }
        Ok(SupervisorOutcome::StderrLimit) => {
            anyhow::bail!("agent stderr exceeded the {MAX_CAPTURE_BYTES}-byte limit")
        }
        Ok(SupervisorOutcome::StdoutLimit) => {
            anyhow::bail!("agent stdout exceeded the {MAX_CAPTURE_BYTES}-byte cumulative limit")
        }
        Err(error) => anyhow::bail!("supervise {}: {error}", agent.display()),
    }
}

/// Non-Unix: no live tap — buffer the run, then deliver it as one chunk.
#[cfg(not(unix))]
fn capture(
    cfg: &AgentConfig,
    resume_id: Option<&str>,
    prompt_and_rest: &[String],
    chunks: mpsc::Sender<Vec<u8>>,
    _tap: &StreamTap,
) -> Result<Ended> {
    let (status, stdout, stderr) = cfg.run_capture(resume_id, prompt_and_rest)?;
    let _ = chunks.send(stdout.into_bytes());
    Ok(Ended::Exited {
        code: status.code().unwrap_or(1),
        success: status.success(),
        stderr,
    })
}

#[cfg(all(test, unix))]
mod tests {
    use super::super::{AgentBackend, AgentConfig};
    use crate::state::AbbeyState;
    use crate::stream::{StreamEvent, StreamTap};
    use std::os::unix::fs::PermissionsExt as _;
    use std::sync::mpsc;

    fn scratch(tag: &str, script: &str) -> (std::path::PathBuf, AbbeyState, AgentConfig) {
        let dir = std::env::temp_dir().join(format!(
            "abbey-stream-{tag}-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("by-cwd")).unwrap();
        let agent = dir.join("agent");
        std::fs::write(&agent, script).unwrap();
        let mut perm = std::fs::metadata(&agent).unwrap().permissions();
        perm.set_mode(0o700);
        std::fs::set_permissions(&agent, perm).unwrap();
        let state = AbbeyState {
            state_dir: dir.clone(),
            chat_file: dir.join("chat-id"),
            model_file: dir.join("model"),
            history_file: dir.join("history.log"),
            cwd_dir: dir.join("by-cwd"),
            per_cwd: false,
            cwd: dir.clone(),
        };
        let cfg = AgentConfig {
            agent_path: agent,
            backend: AgentBackend::Ollama,
            transcript_dir: Some(dir.join("ollama")),
            ..AgentConfig::default()
        };
        (dir, state, cfg)
    }

    fn drain(rx: &mpsc::Receiver<StreamEvent>) -> Vec<StreamEvent> {
        rx.try_iter().collect()
    }

    #[test]
    fn executor_error_event_fails_even_when_the_process_exits_zero() {
        let (dir, state, cfg) = scratch(
            "failed-event",
            "#!/bin/sh\nprintf '%s\\n' '{\"type\":\"result\",\"is_error\":true,\"result\":\"fixture failure\"}'; sleep 30\n",
        );
        let mut cfg = AgentConfig {
            backend: AgentBackend::Claude,
            transcript_dir: Some(dir.join("claude")),
            ..cfg
        };
        state.save_chat("keep-chat").unwrap();
        let (tx, rx) = mpsc::channel();
        let result = crate::actions::run_agent(
            &mut cfg,
            &state,
            &["x".into()],
            crate::actions::RunSpec::resume().streaming(StreamTap::new(tx)),
        );
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("failed stream event")
        );
        assert_eq!(drain(&rx).last(), Some(&StreamEvent::Done { exit: 1 }));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn version_probe_is_bounded_for_slow_or_excessive_executors() {
        for (tag, script) in [
            ("version-slow", "#!/bin/sh\nsleep 30\n"),
            ("version-flood", "#!/bin/sh\nhead -c 8192 /dev/zero\n"),
        ] {
            let (dir, _, cfg) = scratch(tag, script);
            let start = std::time::Instant::now();
            assert_eq!(cfg.agent_version(), "unknown");
            assert!(start.elapsed() < std::time::Duration::from_secs(3));
            let _ = std::fs::remove_dir_all(dir);
        }
    }

    #[test]
    fn decoded_utf8_expansion_overflow_is_failure_without_transcript_commit() {
        let (dir, state, mut cfg) = scratch(
            "decoded-overflow",
            "#!/bin/sh\nhead -c 2097152 /dev/zero | LC_ALL=C tr '\\000' '\\377'\n",
        );
        state.save_chat("keep-chat").unwrap();
        let (tx, rx) = mpsc::channel();
        let result = crate::actions::run_agent(
            &mut cfg,
            &state,
            &["x".into()],
            crate::actions::RunSpec::resume().streaming(StreamTap::new(tx)),
        );
        assert!(result.unwrap_err().to_string().contains("decoded-payload"));
        assert!(!cfg.transcript_path("keep-chat").unwrap().exists());
        let events = drain(&rx);
        assert_eq!(
            events
                .iter()
                .filter(|e| matches!(e, StreamEvent::Failed(_)))
                .count(),
            1
        );
        assert_eq!(events.last(), Some(&StreamEvent::Done { exit: 1 }));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn final_stderr_overflow_cannot_commit_a_successful_turn() {
        let (dir, state, mut cfg) = scratch(
            "stderr-overflow",
            "#!/bin/sh\nprintf ok; head -c 4194304 /dev/zero | tr '\\000' x >&2\n",
        );
        state.save_chat("keep-chat").unwrap();
        let (tx, rx) = mpsc::channel();
        let result = crate::actions::run_agent(
            &mut cfg,
            &state,
            &["x".into()],
            crate::actions::RunSpec::resume().streaming(StreamTap::new(tx)),
        );
        assert!(result.unwrap_err().to_string().contains("decoded-payload"));
        assert!(!cfg.transcript_path("keep-chat").unwrap().exists());
        assert_eq!(drain(&rx).last(), Some(&StreamEvent::Done { exit: 1 }));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn streamed_run_emits_deltas_then_done_and_records_the_transcript() {
        let (dir, state, cfg) = scratch("ok", "#!/bin/sh\nprintf 'hel'; sleep 0.05; printf 'lo'\n");
        let (tx, rx) = mpsc::channel();
        let mut cfg = cfg;
        let code = crate::actions::run_agent(
            &mut cfg,
            &state,
            &["say hi".into()],
            crate::actions::RunSpec::resume().streaming(StreamTap::new(tx)),
        )
        .unwrap();
        assert_eq!(code, 0);
        let events = drain(&rx);
        let text: String = events
            .iter()
            .filter_map(|e| match e {
                StreamEvent::TextDelta(t) => Some(t.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(text, "hello");
        assert_eq!(events.last(), Some(&StreamEvent::Done { exit: 0 }));
        let chat = state
            .resolve_chat_for(AgentBackend::Ollama)
            .unwrap()
            .expect("chat saved");
        let transcript = std::fs::read_to_string(cfg.transcript_path(&chat).unwrap()).unwrap();
        assert!(transcript.contains("hello"));
        let routes = std::fs::read_to_string(dir.join("route.jsonl")).unwrap();
        assert_eq!(
            routes.lines().count(),
            1,
            "exactly one route row per streamed turn"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn streaming_notices_go_to_the_tap_not_stderr() {
        let (dir, state, cfg) = scratch("notice", "#!/bin/sh\nprintf ok\n");
        let (tx, rx) = mpsc::channel();
        let mut cfg = cfg;
        crate::actions::run_agent(
            &mut cfg,
            &state,
            &["x".into()],
            crate::actions::RunSpec::fresh().streaming(StreamTap::new(tx)),
        )
        .unwrap();
        assert!(
            drain(&rx)
                .iter()
                .any(|e| matches!(e, StreamEvent::Notice(n) if n.contains("new chat")))
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cancelled_stream_run_does_not_retry_or_mint_a_chat() {
        let (dir, state, cfg) = scratch("cancel", "#!/bin/sh\nsleep 30\n");
        state.save_chat("keep-me").unwrap();
        let (tx, rx) = mpsc::channel();
        let tap = StreamTap::new(tx);
        tap.cancel.cancel();
        let mut cfg = AgentConfig {
            backend: AgentBackend::Claude,
            transcript_dir: Some(dir.join("claude")),
            ..cfg
        };
        let code = crate::actions::run_agent(
            &mut cfg,
            &state,
            &["x".into()],
            crate::actions::RunSpec::resume().streaming(tap),
        )
        .unwrap();
        assert_eq!(code, 130);
        assert_eq!(drain(&rx).last(), Some(&StreamEvent::Done { exit: 130 }));
        assert_eq!(
            state
                .resolve_chat_for(AgentBackend::Claude)
                .unwrap()
                .as_deref(),
            Some("keep-me")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
