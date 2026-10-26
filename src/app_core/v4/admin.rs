//! Schema 3 bounded records. Data references never grant execution authority.
use super::*;

#[derive(Clone, PartialEq, Eq, Serialize)]
pub struct V4PageQuery {
    pub after: u64,
    pub through: Option<u64>,
    pub limit: u16,
}
impl V4PageQuery {
    pub fn validate(&self) -> Result<(), ValidationError> {
        timestamp(self.after)?;
        limit(self.limit)?;
        if let Some(t) = self.through {
            timestamp(t)?;
            ensure(t >= self.after)?;
        }
        Ok(())
    }
}
validated_admin!(V4PageQuery { after: u64, through: Option<u64>, limit: u16 });

#[derive(Clone, PartialEq, Eq, Serialize)]
pub struct V4SearchQuery {
    pub query: String,
    pub page: V4PageQuery,
}
impl V4SearchQuery {
    pub fn validate(&self) -> Result<(), ValidationError> {
        text(&self.query, MAX_V4_QUERY_BYTES)?;
        self.page.validate()
    }
}
validated_admin!(V4SearchQuery {
    query: String,
    page: V4PageQuery
});

#[derive(Clone, PartialEq, Eq, Serialize)]
pub struct V4ResourceQuery {
    pub resource_id: String,
}
impl V4ResourceQuery {
    pub fn validate(&self) -> Result<(), ValidationError> {
        route(&self.resource_id)
    }
}
validated_admin!(V4ResourceQuery {
    resource_id: String
});

#[derive(Clone, PartialEq, Eq, Serialize)]
pub struct V4ProfileRevision {
    pub profile: V4PermissionProfile,
    pub revision: u64,
    pub digest: String,
    pub revocation_epoch: u64,
}
impl V4ProfileRevision {
    pub fn validate(&self) -> Result<(), ValidationError> {
        self.profile.validate()?;
        positive(self.revision)?;
        digest(&self.digest)?;
        timestamp(self.revocation_epoch)
    }
}
validated_admin!(V4ProfileRevision {
    profile: V4PermissionProfile,
    revision: u64,
    digest: String,
    revocation_epoch: u64
});

#[derive(Clone, PartialEq, Eq, Serialize)]
pub struct V4ProfilePut {
    pub operation_id: IdempotencyKey,
    pub expected_revision: Option<u64>,
    pub profile: V4PermissionProfile,
}
impl V4ProfilePut {
    pub fn validate(&self) -> Result<(), ValidationError> {
        self.profile.validate()?;
        if let Some(r) = self.expected_revision {
            positive(r)?;
        }
        Ok(())
    }
}
validated_admin!(V4ProfilePut { operation_id: IdempotencyKey, expected_revision: Option<u64>, profile: V4PermissionProfile });

#[derive(Clone, PartialEq, Eq, Serialize)]
pub struct V4ArtifactReference {
    pub artifact_id: String,
    pub sha256: String,
    pub byte_length: u64,
    pub media_type: String,
}
impl V4ArtifactReference {
    pub fn validate(&self) -> Result<(), ValidationError> {
        route(&self.artifact_id)?;
        digest(&self.sha256)?;
        timestamp(self.byte_length)?;
        ensure(
            self.media_type.len() <= 128
                && self.media_type.contains('/')
                && self
                    .media_type
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"/.-+".contains(&b)),
        )
    }
}
validated_admin!(V4ArtifactReference {
    artifact_id: String,
    sha256: String,
    byte_length: u64,
    media_type: String
});

/// State and numeric references use the JavaScript exact integer range.
pub(super) fn positive(value: u64) -> Result<(), ValidationError> {
    timestamp(value)?;
    ensure(value > 0)
}
pub(super) fn digest(value: &str) -> Result<(), ValidationError> {
    ensure(
        value.len() == 64
            && value
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
    )
}
pub(super) fn budget(value: u32) -> Result<(), ValidationError> {
    ensure((1..=100_000).contains(&value))
}
pub(super) fn page(after: u64, through: u64, count: usize) -> Result<(), ValidationError> {
    timestamp(after)?;
    timestamp(through)?;
    ensure(after <= through && count <= usize::from(MAX_V4_PAGE))
}
pub(super) fn admin_bytes<T: Serialize>(value: &T) -> Result<(), ValidationError> {
    ensure(
        serde_json::to_vec(value)
            .map_err(|_| ValidationError::new("invalid v4 page"))?
            .len()
            <= 48 * 1024,
    )
}
