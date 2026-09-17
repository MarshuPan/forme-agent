//! Persistent owner-brain authority metadata.
//!
//! These records are companion metadata, not a second fact store. They bind a
//! production runtime to one owner, give the brain stable aggregate ids, and
//! fence concurrent authority writers without fabricating domain events.

use std::collections::BTreeMap;

use forme_protocol as p;
use serde::{Deserialize, Serialize};

pub const BRAIN_AUTHORITY_SCHEMA_VERSION: p::SchemaVersion = p::SchemaVersion(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum BrainAggregateKind {
    MemoryCognition,
    Intentions,
    Charter,
    Dashboard,
    DataLifecycle,
}

impl BrainAggregateKind {
    pub const ALL: [Self; 5] = [
        Self::MemoryCognition,
        Self::Intentions,
        Self::Charter,
        Self::Dashboard,
        Self::DataLifecycle,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::MemoryCognition => "memory_cognition",
            Self::Intentions => "intentions",
            Self::Charter => "charter",
            Self::Dashboard => "dashboard",
            Self::DataLifecycle => "data_lifecycle",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrainAuthorityClaim {
    pub owner_digest: p::SchemaDigest,
    pub process_nonce: p::Nonce,
    pub now_ms: p::Timestamp,
    pub lease_ttl: p::DurationMs,
}

impl BrainAuthorityClaim {
    pub fn validate(&self) -> p::Result<()> {
        p::sha256_digest_bytes(&self.owner_digest)
            .map_err(|_| p::Error("brain authority owner digest is invalid".into()))?;
        if self.process_nonce.0.trim().is_empty() {
            return Err(p::Error(
                "brain authority process nonce is incomplete".into(),
            ));
        }
        if self.process_nonce.0.len() > 256 {
            return Err(p::Error(
                "brain authority process nonce exceeds its bound".into(),
            ));
        }
        if self.now_ms < 0 {
            return Err(p::Error(
                "brain authority claim time cannot be negative".into(),
            ));
        }
        if self.lease_ttl.0 == 0 {
            return Err(p::Error(
                "brain authority writer lease must have a non-zero ttl".into(),
            ));
        }
        self.expires_at().map(|_| ())
    }

    pub fn expires_at(&self) -> p::Result<p::Timestamp> {
        let ttl = i64::try_from(self.lease_ttl.0)
            .map_err(|_| p::Error("brain authority writer lease ttl is invalid".into()))?;
        self.now_ms
            .checked_add(ttl)
            .ok_or_else(|| p::Error("brain authority writer lease expiry is exhausted".into()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorityWriterLease {
    pub store_identity: p::SchemaDigest,
    pub process_nonce: p::Nonce,
    pub expires_at: p::Timestamp,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrainAuthorityBinding {
    pub schema_version: p::SchemaVersion,
    pub owner_digest: p::SchemaDigest,
    pub store_identity: p::SchemaDigest,
    pub created_at: p::Timestamp,
    pub aggregates: BTreeMap<BrainAggregateKind, p::RunId>,
    pub writer_lease: AuthorityWriterLease,
}

impl BrainAuthorityBinding {
    pub fn aggregate(&self, kind: BrainAggregateKind) -> p::Result<p::RunId> {
        self.aggregates.get(&kind).cloned().ok_or_else(|| {
            p::Error(format!(
                "brain authority aggregate {} is missing",
                kind.as_str()
            ))
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrainAuthorityMetadata {
    pub schema_version: p::SchemaVersion,
    pub owner_digest: p::SchemaDigest,
    pub store_identity: p::SchemaDigest,
    pub created_at: p::Timestamp,
    pub aggregates: BTreeMap<BrainAggregateKind, p::RunId>,
}

pub trait BrainAuthorityStore {
    /// Atomically binds the database to one owner, materializes deterministic
    /// aggregate ids, and claims the sole production writer lease.
    fn claim_brain_authority(&self, claim: BrainAuthorityClaim)
        -> p::Result<BrainAuthorityBinding>;

    /// Reads durable owner/aggregate metadata without acquiring authority.
    fn brain_authority(&self) -> p::Result<Option<BrainAuthorityMetadata>>;

    /// Releases only a lease owned by the exact store identity and nonce.
    fn release_brain_authority_lease(
        &self,
        store_identity: &p::SchemaDigest,
        process_nonce: &p::Nonce,
    ) -> p::Result<bool>;
}
