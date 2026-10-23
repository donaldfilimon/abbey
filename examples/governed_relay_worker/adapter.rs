//! Deliberately narrow demonstration: create one startup-selected scratch file.
use abi_agent_host::{
    GovernedFactProvider, GovernedFacts, GovernedLiveExecutor, ObservationSource,
    PostconditionObservation, Postconditions, ToolExecutionContext, ToolExecutionError, ToolOutput,
};
use abi_agent_runtime::ToolCall;
use abi_capability::{
    Clock, Digest, Request,
    lifecycle::{LifecycleError, PreparedExecution},
    source::{AdapterBinding, CompiledSource},
};
use sha2::{Digest as _, Sha256};
use std::{
    cell::RefCell,
    collections::BTreeSet,
    io::{Read, Write},
    path::PathBuf,
    rc::Rc,
};

pub fn digest(bytes: &[u8]) -> Digest {
    Digest::from_bytes(Sha256::digest(bytes).into())
}
pub struct WallClock;
impl Clock for WallClock {
    fn now_ms(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()
            .and_then(|d| u64::try_from(d.as_millis()).ok())
            .unwrap_or(u64::MAX)
    }
}
#[derive(Clone, Default)]
pub struct Evidence(Rc<RefCell<Option<(Digest, Digest)>>>);
pub struct FileAdapter {
    pub path: PathBuf,
    pub evidence: Evidence,
}
impl GovernedLiveExecutor for FileAdapter {
    fn binding(&self) -> AdapterBinding {
        AdapterBinding {
            adapter_id: "scratch-create-new".into(),
            api_version: "1".into(),
        }
    }
    fn execute(
        &mut self,
        call: &ToolCall,
        request: &Request,
        _: &CompiledSource,
        prepared: &PreparedExecution,
        context: ToolExecutionContext<'_>,
    ) -> Result<ToolOutput, ToolExecutionError> {
        if context.should_stop() || prepared.binding().request_digest != request.request_digest {
            return Err(ToolExecutionError::new("dispatch boundary refused"));
        }
        let value: serde_json::Value = serde_json::from_str(&call.input)
            .map_err(|_| ToolExecutionError::new("invalid parameters"))?;
        let content = value["content"]
            .as_str()
            .ok_or_else(|| ToolExecutionError::new("invalid content"))?;
        if content.len() > 1024 {
            return Err(ToolExecutionError::new("content ceiling"));
        }
        let result = (|| -> std::io::Result<()> {
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
            }
            let mut file = options.open(&self.path)?;
            file.write_all(content.as_bytes())?;
            file.sync_all()?;
            std::fs::File::open(
                self.path
                    .parent()
                    .ok_or_else(|| std::io::Error::other("missing parent"))?,
            )?
            .sync_all()?;
            Ok(())
        })();
        result.map_err(|_| ToolExecutionError::new("file effect outcome requires inspection"))?;
        let commitment = digest(content.as_bytes());
        *self.evidence.0.borrow_mut() = Some((request.request_digest, commitment));
        Ok(ToolOutput::ok(
            serde_json::json!({"written":true,"digest":commitment.to_string()}).to_string(),
        ))
    }
}
pub struct FileFacts {
    pub value: GovernedFacts,
    pub path: PathBuf,
    pub evidence: Evidence,
}
impl GovernedFactProvider for FileFacts {
    fn observe(&mut self, _: &Request) -> Result<GovernedFacts, LifecycleError> {
        let now = WallClock.now_ms();
        let mut value = self.value.clone();
        value.observed_at_ms = now;
        value.expires_at_ms = value.expires_at_ms.min(now.saturating_add(10_000));
        if std::fs::symlink_metadata(&self.path).is_ok() {
            value.predicates.remove("target-absent");
        }
        Ok(value)
    }
    fn postconditions(
        &mut self,
        request: &Request,
        required: &BTreeSet<String>,
    ) -> Result<Vec<PostconditionObservation>, LifecycleError> {
        let now = WallClock.now_ms();
        let mut options = std::fs::OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NOFOLLOW);
        }
        let file = options
            .open(&self.path)
            .map_err(|_| LifecycleError::Unavailable)?;
        let metadata = file.metadata().map_err(|_| LifecycleError::Unavailable)?;
        if !metadata.is_file() || metadata.len() > 1024 {
            return Err(LifecycleError::InvalidState);
        }
        let mut bytes = Vec::new();
        file.take(1025)
            .read_to_end(&mut bytes)
            .map_err(|_| LifecycleError::Unavailable)?;
        let actual = digest(&bytes);
        let verified = self.evidence.0.borrow().as_ref() == Some(&(request.request_digest, actual));
        Ok(required
            .iter()
            .map(|predicate| PostconditionObservation {
                predicate: predicate.clone(),
                request_digest: request.request_digest,
                observed_at_ms: now,
                expires_at_ms: now.saturating_add(10_000),
                evidence_digest: Some(actual),
                source: ObservationSource::TrustedLocalAdapter,
                status: if verified {
                    Postconditions::Verified
                } else {
                    Postconditions::Failed
                },
            })
            .collect())
    }
}
