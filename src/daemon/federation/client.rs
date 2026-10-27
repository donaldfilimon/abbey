//! Send-once `abbey.v1` client over the local Unix socket.

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::Duration;
use thiserror::Error;

use super::{
    CONTRACT_MAJOR, CONTRACT_REVISION, CORPUS_DIGEST, FederationMethod, FederationRequest,
    FederationResponse, SERVICE, capability_manifest_digest, parameters_digest,
};

/// Send-once client for the separate contract-governed local interface.
///
/// It has no legacy protocol list, retry loop, or alternate backend.
#[derive(Clone, Debug)]
pub struct FederationClient {
    socket_path: PathBuf,
    timeout: Duration,
}

impl FederationClient {
    #[must_use]
    pub fn new(socket_path: impl Into<PathBuf>) -> Self {
        Self {
            socket_path: socket_path.into(),
            timeout: Duration::from_secs(5),
        }
    }

    pub fn request(
        &self,
        method: FederationMethod,
        parameters: Value,
    ) -> Result<FederationResponse, FederationClientError> {
        let request = FederationRequest {
            service: SERVICE.to_owned(),
            contract_major: CONTRACT_MAJOR,
            contract_revision: CONTRACT_REVISION,
            corpus_digest: CORPUS_DIGEST.to_owned(),
            capability_manifest_digest: capability_manifest_digest(),
            request_id: format!("request_{}", uuid::Uuid::new_v4().simple()),
            method,
            parameters_digest: parameters_digest(&parameters),
            parameters,
        };
        self.request_exact(&request)
    }

    #[cfg(unix)]
    pub fn request_exact(
        &self,
        request: &FederationRequest,
    ) -> Result<FederationResponse, FederationClientError> {
        use std::io::{Read as _, Write as _};
        use std::os::unix::net::UnixStream;

        use super::{MAX_FRAME_BYTES, value_within_limits};

        let bytes = serde_json::to_vec(request).map_err(FederationClientError::Json)?;
        if bytes.is_empty() || bytes.len() > MAX_FRAME_BYTES {
            return Err(FederationClientError::RequestTooLarge);
        }
        let mut stream = UnixStream::connect(&self.socket_path).map_err(|source| {
            FederationClientError::Connect {
                path: self.socket_path.clone(),
                source,
            }
        })?;
        stream
            .set_read_timeout(Some(self.timeout))
            .map_err(FederationClientError::Io)?;
        stream
            .set_write_timeout(Some(self.timeout))
            .map_err(FederationClientError::Io)?;
        stream
            .write_all(&(bytes.len() as u32).to_be_bytes())
            .and_then(|()| stream.write_all(&bytes))
            .and_then(|()| stream.flush())
            .map_err(FederationClientError::Io)?;

        let mut prefix = [0_u8; 4];
        stream
            .read_exact(&mut prefix)
            .map_err(FederationClientError::Io)?;
        let length = u32::from_be_bytes(prefix) as usize;
        if length == 0 || length > MAX_FRAME_BYTES {
            return Err(FederationClientError::ResponseTooLarge);
        }
        let mut response_bytes = vec![0_u8; length];
        stream
            .read_exact(&mut response_bytes)
            .map_err(FederationClientError::Io)?;
        let value: Value =
            serde_json::from_slice(&response_bytes).map_err(FederationClientError::Json)?;
        if !value_within_limits(&value, 1) {
            return Err(FederationClientError::InvalidResponse);
        }
        let response: FederationResponse =
            serde_json::from_value(value).map_err(FederationClientError::Json)?;
        if response.service != SERVICE
            || response.contract_major != CONTRACT_MAJOR
            || response.contract_revision != CONTRACT_REVISION
            || response.request_id != request.request_id
        {
            return Err(FederationClientError::InvalidResponse);
        }
        Ok(response)
    }

    #[cfg(not(unix))]
    pub fn request_exact(
        &self,
        _request: &FederationRequest,
    ) -> Result<FederationResponse, FederationClientError> {
        // abbey.v1 federation over Windows named pipes is not implemented.
        // The transport contract remains Unix-socket only; non-Unix callers
        // receive the explicit UnsupportedPlatform marker.
        Err(FederationClientError::UnsupportedPlatform)
    }

    #[must_use]
    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }
}

#[derive(Debug, Error)]
pub enum FederationClientError {
    #[error("abbey.v1 local transport requires Unix domain sockets")]
    /// Windows named-pipe federation transport stub (see request_exact above).
    UnsupportedPlatform,
    #[error("cannot connect to abbey.v1 socket at {path}: {source}")]
    Connect {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("abbey.v1 I/O failed: {0}")]
    Io(std::io::Error),
    #[error("abbey.v1 JSON failed: {0}")]
    Json(serde_json::Error),
    #[error("abbey.v1 request exceeds the frame limit")]
    RequestTooLarge,
    #[error("abbey.v1 response exceeds the frame limit")]
    ResponseTooLarge,
    #[error("abbey.v1 response failed correlation or structural validation")]
    InvalidResponse,
}
