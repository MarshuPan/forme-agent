//! Persistent owner-brain authority metadata.
//!
//! These records are companion metadata, not a second fact store. They bind a
//! production runtime to one owner, give the brain stable aggregate ids, and
//! fence concurrent authority writers without fabricating domain events.

use std::{collections::BTreeMap, fmt, sync::Arc};

use forme_protocol as p;
use serde::{Deserialize, Serialize};

use crate::SqliteEventStore;

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

/// Runtime-owned authority fence shared by every clone of the production
/// store. The exact nonce is intentionally not exposed through Debug or any
/// portable runtime manifest.
#[derive(Clone)]
pub struct AuthorityLeaseGuard {
    pub(crate) inner: Arc<AuthorityLeaseGuardInner>,
}

pub(crate) struct AuthorityLeaseGuardInner {
    pub(crate) store: SqliteEventStore,
    pub(crate) owner_digest: p::SchemaDigest,
    pub(crate) store_identity: p::SchemaDigest,
    pub(crate) process_nonce: p::Nonce,
    pub(crate) lease_ttl: p::DurationMs,
}

impl AuthorityLeaseGuard {
    pub(crate) fn new(
        store: SqliteEventStore,
        binding: &BrainAuthorityBinding,
        lease_ttl: p::DurationMs,
    ) -> p::Result<Self> {
        if lease_ttl.0 == 0
            || binding.schema_version != BRAIN_AUTHORITY_SCHEMA_VERSION
            || binding.owner_digest.0.trim().is_empty()
            || binding.store_identity.0.trim().is_empty()
            || binding.writer_lease.store_identity != binding.store_identity
            || binding.writer_lease.process_nonce.0.trim().is_empty()
        {
            return Err(p::Error(
                "brain authority lease guard configuration is incomplete".into(),
            ));
        }
        Ok(Self {
            inner: Arc::new(AuthorityLeaseGuardInner {
                store,
                owner_digest: binding.owner_digest.clone(),
                store_identity: binding.store_identity.clone(),
                process_nonce: binding.writer_lease.process_nonce.clone(),
                lease_ttl,
            }),
        })
    }

    /// Renews the exact lease and verifies that the durable owner/store
    /// identity still matches this runtime. A takeover by another nonce is a
    /// hard failure; callers must not retry the pending mutation elsewhere.
    pub fn ensure(&self, now_ms: p::Timestamp) -> p::Result<AuthorityWriterLease> {
        self.inner.store.renew_brain_authority_lease(
            &self.inner.owner_digest,
            &self.inner.store_identity,
            &self.inner.process_nonce,
            now_ms,
            self.inner.lease_ttl,
        )
    }

    pub fn owner_digest(&self) -> &p::SchemaDigest {
        &self.inner.owner_digest
    }

    pub fn store_identity(&self) -> &p::SchemaDigest {
        &self.inner.store_identity
    }
}

impl fmt::Debug for AuthorityLeaseGuard {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AuthorityLeaseGuard")
            .field("owner_digest", &self.inner.owner_digest)
            .field("store_identity", &self.inner.store_identity)
            .field("process_nonce", &"<redacted>")
            .field("lease_ttl", &self.inner.lease_ttl)
            .finish()
    }
}

impl Drop for AuthorityLeaseGuardInner {
    fn drop(&mut self) {
        let _ = self
            .store
            .release_brain_authority_lease(&self.store_identity, &self.process_nonce);
    }
}

pub trait BrainAuthorityStore {
    /// Atomically binds the database to one owner, materializes deterministic
    /// aggregate ids, and claims the sole production writer lease.
    fn claim_brain_authority(&self, claim: BrainAuthorityClaim)
        -> p::Result<BrainAuthorityBinding>;

    /// Reads durable owner/aggregate metadata without acquiring authority.
    fn brain_authority(&self) -> p::Result<Option<BrainAuthorityMetadata>>;

    /// Renews only the exact active writer. This never creates a missing
    /// lease and therefore cannot silently resurrect a released runtime.
    fn renew_brain_authority_lease(
        &self,
        owner_digest: &p::SchemaDigest,
        store_identity: &p::SchemaDigest,
        process_nonce: &p::Nonce,
        now_ms: p::Timestamp,
        lease_ttl: p::DurationMs,
    ) -> p::Result<AuthorityWriterLease>;

    /// Releases only a lease owned by the exact store identity and nonce.
    fn release_brain_authority_lease(
        &self,
        store_identity: &p::SchemaDigest,
        process_nonce: &p::Nonce,
    ) -> p::Result<bool>;
}
