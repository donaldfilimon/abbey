//! One optional local rerank per unchanged draft, with observed worker joins.

use super::app::SHUTDOWN_JOIN_GRACE;
use super::predict;
use super::worker::join_within;
use crate::agent::{AgentBackend, AgentConfig};
use crate::runtime::CancellationToken;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, TryRecvError};
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

impl Worker {
    /// Whether the worker has finished.
    ///
    /// The thread's own flag is set only after the closure has returned,
    /// which is a moment after the worker did its work. The result
    /// channel is the earlier signal: the worker sends its answer and
    /// then drops the sender, so a channel that is drained and
    /// disconnected means the worker is done.
    ///
    /// The buffered answer is drained here. A detached worker's answer is
    /// discarded anyway — it answered a draft that is no longer the
    /// draft — and a drained channel is what tells a finished worker
    /// apart from a running one.
    fn has_finished(&self) -> bool {
        if self.thread.is_finished() {
            return true;
        }
        loop {
            match self.result.try_recv() {
                Ok(_) => continue,
                Err(TryRecvError::Empty) => return false,
                Err(TryRecvError::Disconnected) => return true,
            }
        }
    }
}

#[derive(Default)]
pub(crate) struct PredictionOwner {
    observed: Option<Stamp>,
    attempted: Option<Draft>,
    changed_tick: u64,
    worker: Option<Worker>,
    /// Workers that were cancelled and left to finish on their own.
    ///
    /// The UI thread must not block on a worker: the rerank runs
    /// `ollama list` and `ollama run` under the supervisor, so joining it
    /// would stall every keystroke for as long as the rerank takes. A
    /// cancelled worker is detached here and reaped once it has finished.
    detached: Vec<Worker>,
    boost: Option<&'static str>,
    /// Workers that did not stop inside the shutdown grace.
    ///
    /// A worker that will not stop is detached rather than waited for:
    /// the app is leaving, and a wedged rerank must not keep the
    /// terminal in raw mode. The count is reported by the app, so the
    /// user is not shown a quiet success.
    abandoned: usize,
}

impl PredictionOwner {
    /// Cancel the live worker and let it finish on its own.
    ///
    /// The worker is detached rather than joined: the UI thread cannot
    /// wait for a rerank it has already decided is stale.
    fn detach(&mut self) {
        if let Some(worker) = self.worker.take() {
            worker.cancel.cancel();
            self.detached.push(worker);
        }
    }

    /// Reap the detached workers that have finished.
    ///
    /// A finished worker is joined — that cannot block — and its answer is
    /// discarded: it answered a draft that is no longer the draft. A
    /// worker that has sent its answer but whose thread is not marked
    /// finished yet is dropped instead of joined: the UI thread must not
    /// wait for a worker.
    fn reap(&mut self) {
        let mut running = Vec::new();
        for worker in std::mem::take(&mut self.detached) {
            if worker.thread.is_finished() {
                let _ = worker.thread.join();
            } else if !worker.has_finished() {
                running.push(worker);
            }
        }
        self.detached = running;
    }

    /// Take the live worker once it has finished.
    fn take_finished(&mut self) -> anyhow::Result<Option<(Stamp, Option<&'static str>)>> {
        let finished = self
            .worker
            .as_ref()
            .is_some_and(|worker| worker.thread.is_finished());
        if !finished {
            return Ok(None);
        }
        let worker = self.worker.take().expect("a finished worker");
        let hint = worker
            .result
            .try_recv()
            .map_err(|_| anyhow::anyhow!("prediction worker returned no outcome"))??;
        Ok(Some((worker.stamp, hint)))
    }

    /// Cancel every worker and wait for it.
    ///
    /// Used at shutdown: the app is leaving, so nothing is stale any
    /// more, and the workers must not outlive it.
    ///
    /// The wait is bounded: a worker that will not stop is detached
    /// rather than waited for, so the app can leave with the terminal
    /// still in raw mode. A worker that panicked is an error; a worker
    /// that missed the bound is counted, and the app reports the count.
    fn join_all(&mut self) -> anyhow::Result<()> {
        self.detach();
        let mut workers = std::mem::take(&mut self.detached);
        if let Some(worker) = self.worker.take() {
            workers.push(worker);
        }
        for worker in workers {
            let Worker { cancel, thread, .. } = worker;
            cancel.cancel();
            if !join_within(&thread, SHUTDOWN_JOIN_GRACE) {
                // The worker will not stop. It is detached: the handle
                // is dropped, the thread finishes on its own, and its
                // answer goes nowhere.
                self.abandoned += 1;
                continue;
            }
            thread
                .join()
                .map_err(|_| anyhow::anyhow!("prediction worker panicked"))?;
        }
        Ok(())
    }

    /// Drop the current hint and stop the worker that may produce one.
    ///
    /// The worker is cancelled and detached, not joined: this runs on the
    /// UI thread, on a keypress.
    pub(crate) fn dismiss(&mut self) -> anyhow::Result<()> {
        self.attempted = self.observed.as_ref().map(|stamp| stamp.draft.clone());
        self.boost = None;
        self.detach();
        self.reap();
        Ok(())
    }

    /// Cancel every worker and wait for it, at shutdown.
    pub(crate) fn shutdown(&mut self) -> anyhow::Result<()> {
        self.boost = None;
        self.join_all()
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
            // The draft changed: the worker in flight answered a draft
            // that is gone. It is cancelled and detached, never joined on
            // the UI thread.
            self.detach();
            self.reap();
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
        self.reap();
        if let Some((completed, hint)) = self.take_finished()?
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

    /// How many workers were abandoned instead of joined.
    ///
    /// A worker that did not stop inside the shutdown grace is detached,
    /// not waited for. The app reports the count, so the user learns
    /// that a background worker outlived the app instead of seeing a
    /// quiet success.
    pub(crate) fn abandoned(&self) -> usize {
        self.abandoned
    }
}

impl Drop for PredictionOwner {
    fn drop(&mut self) {
        let _ = self.join_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cancelled_worker_is_detached_and_never_publishes_its_hint() {
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
        // The draft changed: the worker is cancelled and detached, and
        // the UI thread is not made to wait for it.
        let started = std::time::Instant::now();
        owner.sync("/mem", 4, &cfg, 1, true).unwrap();
        assert!(started.elapsed() < std::time::Duration::from_millis(50));
        assert!(owner.worker.is_none());
        assert!(owner.boost().is_none());
        assert!(
            owner.pump("/mem", 4, &cfg, 1, true).unwrap().is_none(),
            "the stale hint must not be published"
        );
    }

    #[test]
    fn a_detached_worker_is_reaped_once_it_has_finished() {
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
        // The worker is held until the test releases it, so the detach is
        // observed while it is still running, and the reap is observed
        // after it has finished. The signal says the worker is done, and
        // the sender is dropped before it, so the result channel is
        // disconnected by the time the test reaps.
        let (release_tx, release) = mpsc::channel();
        let (done_tx, done) = mpsc::channel();
        let thread = std::thread::spawn(move || {
            let _ = release.recv();
            drop(tx);
            let _ = done_tx.send(());
        });
        owner.worker = Some(Worker {
            stamp: old,
            cancel: CancellationToken::new(),
            result,
            thread,
        });
        owner.dismiss().unwrap();
        assert_eq!(owner.detached.len(), 1, "the worker is detached");
        release_tx.send(()).unwrap();
        done.recv().unwrap();
        owner.reap();
        assert!(owner.detached.is_empty(), "a finished worker is reaped");
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
