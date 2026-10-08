//! One optional local rerank per unchanged draft, with observed worker joins.

use super::predict;
use crate::agent::{AgentBackend, AgentConfig};
use crate::runtime::CancellationToken;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::thread::JoinHandle;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Draft {
    text: String,
    backend: AgentBackend,
    executable: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Stamp {
    draft: Draft,
    cursor: usize,
}

struct Worker {
    stamp: Stamp,
    cancel: CancellationToken,
    result: Receiver<anyhow::Result<Option<&'static str>>>,
    thread: JoinHandle<()>,
}

#[derive(Default)]
pub(crate) struct PredictionOwner {
    observed: Option<Stamp>,
    attempted: Option<Draft>,
    changed_tick: u64,
    worker: Option<Worker>,
    boost: Option<&'static str>,
}

impl PredictionOwner {
    fn join(&mut self, cancel: bool) -> anyhow::Result<Option<(Stamp, Option<&'static str>)>> {
        let Some(worker) = self.worker.take() else {
            return Ok(None);
        };
        if cancel {
            worker.cancel.cancel();
        }
        worker
            .thread
            .join()
            .map_err(|_| anyhow::anyhow!("prediction worker panicked"))?;
        let hint = worker
            .result
            .try_recv()
            .map_err(|_| anyhow::anyhow!("prediction worker returned no outcome"))??;
        Ok(Some((worker.stamp, hint)))
    }

    pub(crate) fn dismiss(&mut self) -> anyhow::Result<()> {
        self.attempted = self.observed.as_ref().map(|stamp| stamp.draft.clone());
        self.boost = None;
        self.join(true)?;
        Ok(())
    }

    pub(crate) fn sync(
        &mut self,
        text: &str,
        cursor: usize,
        cfg: &AgentConfig,
        tick: u64,
        allowed: bool,
    ) -> anyhow::Result<()> {
        let stamp = Stamp {
            draft: Draft {
                text: text.into(),
                backend: cfg.backend,
                executable: cfg.agent_path.clone(),
            },
            cursor,
        };
        if self.observed.as_ref() != Some(&stamp) {
            self.join(true)?;
            if self
                .observed
                .as_ref()
                .is_none_or(|old| old.draft != stamp.draft)
            {
                self.attempted = None;
            }
            self.observed = Some(stamp);
            self.boost = None;
            self.changed_tick = tick;
        }
        if !allowed && (self.worker.is_some() || self.boost.is_some()) {
            self.dismiss()?;
        }
        Ok(())
    }

    pub(crate) fn pump(
        &mut self,
        text: &str,
        cursor: usize,
        cfg: &AgentConfig,
        tick: u64,
        allowed: bool,
    ) -> anyhow::Result<Option<&'static str>> {
        self.sync(text, cursor, cfg, tick, allowed)?;
        if self
            .worker
            .as_ref()
            .is_some_and(|worker| worker.thread.is_finished())
            && let Some((completed, hint)) = self.join(false)?
            && self.observed.as_ref() == Some(&completed)
            && allowed
        {
            self.boost = hint;
            return Ok(hint);
        }
        let stamp = self.observed.as_ref().expect("observed draft");
        if allowed
            && cfg.backend == AgentBackend::Ollama
            && !cfg.agent_path.as_os_str().is_empty()
            && cursor == text.len()
            && text.trim().len() >= 4
            && !text.contains('\n')
            && !text.contains('@')
            && !(text.trim_start().starts_with('/')
                && text.trim_start().contains(char::is_whitespace))
            && tick.saturating_sub(self.changed_tick) >= 8
            && self.attempted.as_ref() != Some(&stamp.draft)
            && self.worker.is_none()
        {
            let stamp = stamp.clone();
            self.attempted = Some(stamp.draft.clone());
            let cancel = CancellationToken::new();
            let token = cancel.clone();
            let (tx, result) = mpsc::channel();
            let draft = stamp.draft.clone();
            let thread = std::thread::Builder::new()
                .name("abbey-tui-predict".into())
                .spawn(move || {
                    let hint = predict::llm_hint_owned(
                        &draft.executable,
                        predict::PREDICT_MODEL,
                        &draft.text,
                        &token,
                    );
                    let _ = tx.send(hint);
                })?;
            self.worker = Some(Worker {
                stamp,
                cancel,
                result,
                thread,
            });
        }
        Ok(None)
    }

    pub(crate) fn boost(&self) -> Option<&'static str> {
        self.boost
    }
}

impl Drop for PredictionOwner {
    fn drop(&mut self) {
        let _ = self.join(true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queued_old_hint_is_joined_and_discarded_after_a_draft_change() {
        let cfg = AgentConfig::fixed_provider_recipe(
            "/owned/ollama".into(),
            AgentBackend::Ollama,
            "local".into(),
        );
        let mut owner = PredictionOwner::default();
        owner.sync("/rev", 4, &cfg, 0, true).unwrap();
        let old = owner.observed.clone().unwrap();
        let (tx, result) = mpsc::channel();
        tx.send(Ok(Some("doctor"))).unwrap();
        let thread = std::thread::spawn(move || drop(tx));
        owner.worker = Some(Worker {
            stamp: old,
            cancel: CancellationToken::new(),
            result,
            thread,
        });
        owner.sync("/mem", 4, &cfg, 1, true).unwrap();
        assert!(owner.worker.is_none());
        assert!(owner.boost().is_none());
        assert!(owner.pump("/mem", 4, &cfg, 1, true).unwrap().is_none());
    }

    #[test]
    fn dismissing_an_already_queued_hint_cannot_publish_or_rearm_it() {
        let cfg = AgentConfig::fixed_provider_recipe(
            "/owned/ollama".into(),
            AgentBackend::Ollama,
            "local".into(),
        );
        let mut owner = PredictionOwner::default();
        owner.sync("/rev", 4, &cfg, 0, true).unwrap();
        let stamp = owner.observed.clone().unwrap();
        let (tx, result) = mpsc::channel();
        tx.send(Ok(Some("review"))).unwrap();
        owner.worker = Some(Worker {
            stamp,
            cancel: CancellationToken::new(),
            result,
            thread: std::thread::spawn(move || drop(tx)),
        });
        owner.dismiss().unwrap();
        assert!(owner.pump("/rev", 4, &cfg, 100, true).unwrap().is_none());
        assert!(owner.worker.is_none());
    }
}
