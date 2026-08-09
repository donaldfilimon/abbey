//! Opaque conversation-identity material for canonical runtime metadata.
//!
//! Raw provider conversation identifiers and working-directory paths are
//! accepted only long enough to validate and hash them. Callers retain the
//! backward-compatible mirror; `runtime.sqlite` stores these derived values.

use crate::app_core::ConversationId;
use sha2::{Digest, Sha256};
use std::fmt;
use std::path::Path;
use thiserror::Error;

const MAX_EXTERNAL_ID_BYTES: usize = 512;
const MAX_EDITION_SLUG_BYTES: usize = 64;
const MAX_MUTATION_TOKEN_BYTES: usize = 128;
const UUID_DOMAIN: &[u8] = b"abbey:legacy-conversation:v1\0";
const ALIAS_DOMAIN: &[u8] = b"abbey:legacy-conversation-alias:v1\0";
const EDITION_DOMAIN: &[u8] = b"abbey:conversation-identity-edition:v1\0";
const GLOBAL_SCOPE_DOMAIN: &[u8] = b"abbey:conversation-identity-scope:global:v1\0";
const CWD_SCOPE_DOMAIN: &[u8] = b"abbey:conversation-identity-scope:cwd:v1\0";
const MUTATION_DOMAIN: &[u8] = b"abbey:conversation-identity-mutation:v1\0";
const SCOPE_SET_DOMAIN: &[u8] = b"abbey:conversation-identity-scope-set:v1\0";
const MAX_IDENTITY_SCOPES: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub(crate) enum IdentityError {
    #[error("external conversation identity is empty, contains controls, or exceeds 512 bytes")]
    ExternalId,
    #[error("edition identity is empty, contains controls, or exceeds 64 bytes")]
    Edition,
    #[error("identity mutation token is empty, contains controls, or exceeds 128 bytes")]
    MutationToken,
    #[error("identity scope set is empty, duplicated, or exceeds 16 scopes")]
    ScopeSet,
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct ExternalIdentity {
    pub(crate) alias_sha256: String,
    pub(crate) conversation_id: ConversationId,
}

impl fmt::Debug for ExternalIdentity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExternalIdentity")
            .field("alias_sha256", &self.alias_sha256)
            .field("conversation_id", &self.conversation_id)
            .finish()
    }
}

/// Opaque digest of one edition-local identity-selection scope.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct ConversationIdentityScope {
    sha256: String,
}

impl fmt::Debug for ConversationIdentityScope {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ConversationIdentityScope")
            .field("sha256", &self.sha256)
            .finish()
    }
}

impl ConversationIdentityScope {
    #[must_use]
    pub(crate) fn global() -> Self {
        Self {
            sha256: digest_parts(GLOBAL_SCOPE_DOMAIN, &[]),
        }
    }

    #[must_use]
    pub(crate) fn working_directory(path: &Path) -> Self {
        Self {
            sha256: digest_parts(CWD_SCOPE_DOMAIN, scope_path_bytes(path).as_ref()),
        }
    }

    #[must_use]
    pub(crate) fn as_sha256(&self) -> &str {
        &self.sha256
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IdentityOperation {
    Save,
}

impl IdentityOperation {
    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "save" => Some(Self::Save),
            _ => None,
        }
    }
}

/// Opaque durable marker for the last committed identity mutation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct IdentityCommit {
    pub(crate) revision: u64,
    pub(crate) operation: IdentityOperation,
    pub(crate) edition_sha256: String,
    pub(crate) scope_sha256: String,
    pub(crate) scope_set_sha256: String,
    pub(crate) alias_sha256: String,
    pub(crate) conversation_id: ConversationId,
    pub(crate) mutation_sha256: String,
    pub(crate) committed_at: String,
}

impl IdentityCommit {
    /// Authenticate one caller-retained prepared mirror journal against this
    /// opaque canonical commit without exposing private hash helpers.
    #[must_use]
    pub(crate) fn matches_save_scopes(
        &self,
        edition_slug: &str,
        scopes: &[ConversationIdentityScope],
        external_id: &str,
        mutation_token: &str,
    ) -> bool {
        let Ok(edition_sha256) = edition_sha256(edition_slug) else {
            return false;
        };
        let Ok(external) = external_identity(external_id) else {
            return false;
        };
        let Ok(mutation_sha256) = mutation_sha256(mutation_token) else {
            return false;
        };
        let Ok(scope_set_sha256) = scope_set_sha256(scopes) else {
            return false;
        };
        self.operation == IdentityOperation::Save
            && self.edition_sha256 == edition_sha256
            && self.scope_sha256 == scopes[0].as_sha256()
            && self.scope_set_sha256 == scope_set_sha256
            && self.alias_sha256 == external.alias_sha256
            && self.conversation_id == external.conversation_id
            && self.mutation_sha256 == mutation_sha256
    }
}

pub(crate) fn external_identity(value: &str) -> Result<ExternalIdentity, IdentityError> {
    let value = validate_external_id(value)?;
    let digest = digest_bytes(UUID_DOMAIN, value.as_bytes());
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x80;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let conversation_id = uuid::Uuid::from_bytes(bytes)
        .to_string()
        .parse()
        .expect("UUIDv8 is a canonical conversation id");
    Ok(ExternalIdentity {
        alias_sha256: lower_hex(&digest_bytes(ALIAS_DOMAIN, value.as_bytes())),
        conversation_id,
    })
}

pub(crate) fn edition_sha256(slug: &str) -> Result<String, IdentityError> {
    if slug.is_empty() || slug.len() > MAX_EDITION_SLUG_BYTES || slug.chars().any(char::is_control)
    {
        return Err(IdentityError::Edition);
    }
    Ok(digest_parts(EDITION_DOMAIN, slug.as_bytes()))
}

pub(crate) fn mutation_sha256(token: &str) -> Result<String, IdentityError> {
    if token.is_empty()
        || token.len() > MAX_MUTATION_TOKEN_BYTES
        || token.chars().any(char::is_control)
    {
        return Err(IdentityError::MutationToken);
    }
    Ok(digest_parts(MUTATION_DOMAIN, token.as_bytes()))
}

pub(crate) fn scope_set_sha256(
    scopes: &[ConversationIdentityScope],
) -> Result<String, IdentityError> {
    if scopes.is_empty() || scopes.len() > MAX_IDENTITY_SCOPES {
        return Err(IdentityError::ScopeSet);
    }
    for (index, scope) in scopes.iter().enumerate() {
        if scopes[..index].iter().any(|prior| prior == scope) {
            return Err(IdentityError::ScopeSet);
        }
    }
    let mut digest = Sha256::new();
    digest.update(SCOPE_SET_DOMAIN);
    digest.update((scopes.len() as u64).to_be_bytes());
    for scope in scopes {
        digest.update(scope.as_sha256().as_bytes());
    }
    Ok(lower_hex(&digest.finalize()))
}

#[must_use]
pub(crate) fn is_lower_hex_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn validate_external_id(value: &str) -> Result<&str, IdentityError> {
    let value = value.trim();
    if value.is_empty()
        || value.len() > MAX_EXTERNAL_ID_BYTES
        || value.chars().any(char::is_control)
    {
        Err(IdentityError::ExternalId)
    } else {
        Ok(value)
    }
}

fn digest_parts(domain: &[u8], material: &[u8]) -> String {
    lower_hex(&digest_bytes(domain, material))
}

fn digest_bytes(domain: &[u8], material: &[u8]) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(domain);
    digest.update(material);
    digest.finalize().into()
}

fn lower_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    encoded
}

#[cfg(unix)]
fn scope_path_bytes(path: &Path) -> std::borrow::Cow<'_, [u8]> {
    use std::os::unix::ffi::OsStrExt as _;
    std::borrow::Cow::Borrowed(path.as_os_str().as_bytes())
}

#[cfg(windows)]
fn scope_path_bytes(path: &Path) -> std::borrow::Cow<'_, [u8]> {
    use std::os::windows::ffi::OsStrExt as _;
    let bytes = path
        .as_os_str()
        .encode_wide()
        .flat_map(u16::to_le_bytes)
        .collect();
    std::borrow::Cow::Owned(bytes)
}

#[cfg(not(any(unix, windows)))]
fn scope_path_bytes(path: &Path) -> std::borrow::Cow<'_, [u8]> {
    std::borrow::Cow::Owned(path.to_string_lossy().into_owned().into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn external_identity_is_v2_byte_compatible_and_bounded() {
        let identity = external_identity(" private-chat-id ").unwrap();
        assert_eq!(
            identity.conversation_id.as_str(),
            "5731a3a9-ba84-83a0-85ca-a5c45a44362e"
        );
        assert_eq!(identity.alias_sha256.len(), 64);
        assert_eq!(identity.conversation_id.as_str().as_bytes()[14], b'8');
        assert!(external_identity("").is_err());
        assert!(external_identity("bad\nid").is_err());
        assert!(external_identity(&"x".repeat(513)).is_err());
    }

    #[test]
    fn edition_and_scope_material_is_domain_separated_and_opaque() {
        assert_ne!(
            edition_sha256("abbey").unwrap(),
            edition_sha256("abbey-personal").unwrap()
        );
        let global = ConversationIdentityScope::global();
        let cwd = ConversationIdentityScope::working_directory(Path::new("/private/project"));
        assert_ne!(global, cwd);
        assert_eq!(cwd.as_sha256().len(), 64);
        assert!(!format!("{cwd:?}").contains("private"));
    }
}
