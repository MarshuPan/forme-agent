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
pub const COGNITIVE_TICK_SCHEMA_VERSION: p::SchemaVersion = p::SchemaVersion(1);
const MAX_TICK_RANGE_BYTES: usize = 1_048_576;

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

/// Per-run stream positions. A tick reads, for each run, the events whose
/// `stream_seq` lies after `from` and at or before `to`; `stream_seq` stays
/// the only ordering authority.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TickEventCursor(pub BTreeMap<p::RunId, u64>);

impl TickEventCursor {
    pub fn position(&self, run: &p::RunId) -> u64 {
        self.0.get(run).copied().unwrap_or(0)
    }
}

/// The fixed event range one tick window reads.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TickEventRange {
    pub from: TickEventCursor,
    pub to: TickEventCursor,
}

impl TickEventRange {
    pub fn validate(&self) -> p::Result<()> {
        for (run, from) in &self.from.0 {
            if run.0.trim().is_empty() || *from > self.to.position(run) {
                return Err(p::Error(
                    "tick event range must never move a run backwards".into(),
                ));
            }
        }
        if self.to.0.keys().any(|run| run.0.trim().is_empty()) {
            return Err(p::Error("tick event range names an empty run".into()));
        }
        Ok(())
    }

    /// True when no run advanced between `from` and `to`.
    pub fn is_empty(&self) -> bool {
        self.to
            .0
            .iter()
            .all(|(run, to)| *to <= self.from.position(run))
    }

    pub fn contains(&self, run: &p::RunId, stream_seq: u64) -> bool {
        stream_seq > self.from.position(run) && stream_seq <= self.to.position(run)
    }

    pub(crate) fn encode(&self) -> p::Result<String> {
        self.validate()?;
        let encoded = serde_json::to_string(self)
            .map_err(|_| p::Error("tick event range cannot be encoded".into()))?;
        if encoded.len() > MAX_TICK_RANGE_BYTES {
            return Err(p::Error("tick event range exceeds its bound".into()));
        }
        Ok(encoded)
    }

    pub(crate) fn decode(value: &str) -> p::Result<Self> {
        let range: Self = serde_json::from_str(value)
            .map_err(|_| p::Error("stored tick event range is invalid".into()))?;
        range.validate()?;
        Ok(range)
    }
}

/// A request to claim one owner-brain tick window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CognitiveTickClaimRequest {
    pub owner_digest: p::SchemaDigest,
    pub window: p::Timestamp,
    pub lease_nonce: p::Nonce,
    pub now_ms: p::Timestamp,
    pub lease_ttl: p::DurationMs,
    /// The range this window reads. Only the first claim records it; a later
    /// takeover of the same window reads the recorded range, so a retried
    /// tick sees exactly the inputs of the attempt it replaces.
    pub range: TickEventRange,
    /// The tick cadence. A new window opens only after every earlier window
    /// finished and at least this long after the latest one began, so a
    /// duplicate signal inside the cadence never opens a second window.
    pub min_spacing: p::DurationMs,
}

impl CognitiveTickClaimRequest {
    pub fn validate(&self) -> p::Result<()> {
        p::sha256_digest_bytes(&self.owner_digest)
            .map_err(|_| p::Error("tick claim owner digest is invalid".into()))?;
        if i64::try_from(self.min_spacing.0).is_err() {
            return Err(p::Error("tick claim cadence is invalid".into()));
        }
        if self.window < 0
            || self.now_ms < 0
            || self.lease_ttl.0 == 0
            || self.lease_nonce.0.trim().is_empty()
            || self.lease_nonce.0.len() > 256
        {
            return Err(p::Error("tick claim request is incomplete".into()));
        }
        self.range.validate()?;
        self.expires_at().map(|_| ())
    }

    pub fn expires_at(&self) -> p::Result<p::Timestamp> {
        let ttl = i64::try_from(self.lease_ttl.0)
            .map_err(|_| p::Error("tick claim lease ttl is invalid".into()))?;
        self.now_ms
            .checked_add(ttl)
            .ok_or_else(|| p::Error("tick claim lease expiry is exhausted".into()))
    }
}

/// An active claim on one tick window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CognitiveTickClaim {
    pub schema_version: p::SchemaVersion,
    pub owner_digest: p::SchemaDigest,
    pub window: p::Timestamp,
    pub lease_nonce: p::Nonce,
    pub attempt: u32,
    pub expires_at: p::Timestamp,
    pub range: TickEventRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CognitiveTickWindowState {
    Claimed {
        attempt: u32,
        expires_at: p::Timestamp,
    },
    Finished {
        attempt: u32,
        report_digest: p::SchemaDigest,
        consumed: TickEventCursor,
    },
    Abandoned {
        attempt: u32,
    },
}

/// Durable claim/lease ledger for owner-brain ticks (prd/23 B1). A window is
/// fulfilled at most once; duplicate signals observe the existing claim.
/// Windows are strictly sequential: a new window opens only after the latest
/// one finished, so an unfinished window is always retried before any newer
/// window reads past it.
pub trait CognitiveTickLedger {
    /// Claims a window. `None` means it is already finished, held by a live
    /// claim, not yet due under the cadence, or blocked by an unfinished
    /// earlier window, so the caller must not run it.
    fn claim_tick(
        &self,
        request: CognitiveTickClaimRequest,
    ) -> p::Result<Option<CognitiveTickClaim>>;

    /// Finishes the exact claim, consuming its whole range.
    fn finish_tick(
        &self,
        claim: &CognitiveTickClaim,
        report_digest: p::SchemaDigest,
    ) -> p::Result<()>;

    /// Finishes the exact claim, consuming only `consumed` (for example, the
    /// start of the range when heavy phases deferred to the foreground).
    fn finish_tick_consuming(
        &self,
        claim: &CognitiveTickClaim,
        report_digest: p::SchemaDigest,
        consumed: TickEventCursor,
    ) -> p::Result<()>;

    /// Marks expired unfinished claims abandoned so their windows can retry.
    fn abandon_expired_tick(
        &self,
        owner_digest: &p::SchemaDigest,
        now_ms: p::Timestamp,
    ) -> p::Result<()>;

    fn tick_window(
        &self,
        owner_digest: &p::SchemaDigest,
        window: p::Timestamp,
    ) -> p::Result<Option<CognitiveTickWindowState>>;

    /// The newest window and its state; the only window that can be
    /// unfinished, because windows are strictly sequential.
    fn latest_tick_window(
        &self,
        owner_digest: &p::SchemaDigest,
    ) -> p::Result<Option<(p::Timestamp, CognitiveTickWindowState)>>;

    /// Everything finished windows consumed, per run. Each window records
    /// only the runs it read, so the durable cursor grows by run, never by
    /// window, and a window range stays bounded by its own activity.
    fn consumed_tick_cursor(&self, owner_digest: &p::SchemaDigest) -> p::Result<TickEventCursor>;
}
