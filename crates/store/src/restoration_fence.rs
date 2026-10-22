//! Restore boundaries are installation/authority metadata, never brain events.
//! Historical events remain readable; their old execution authority does not.
use crate::{offline_validation::invalid, OfflineBrainIdentity, SqliteEventStore};
use forme_protocol as p;
use rusqlite::{params, Connection, TransactionBehavior};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

pub(crate) const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS brain_restore_fences (
    generation INTEGER PRIMARY KEY CHECK(generation > 0),
    payload BLOB NOT NULL,
    digest TEXT NOT NULL UNIQUE
);
"#;
const MAX_FENCE_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RestoredExecutionFence {
    pub schema_version: p::SchemaVersion,
    pub generation: u64,
    pub owner_digest: p::SchemaDigest,
    pub store_identity: p::SchemaDigest,
    pub source_database_digest: p::SchemaDigest,
    pub plan_digest: p::SchemaDigest,
    pub previous_digest: Option<p::SchemaDigest>,
    /// Positions are per stream. A maximum sequence is not a global cursor.
    pub streams: BTreeMap<p::RunId, u64>,
}

impl RestoredExecutionFence {
    pub fn digest(&self) -> p::Result<p::SchemaDigest> {
        p::canonical_digest(&("forme-restored-execution-fence-v1", self))
    }

    pub fn contains(&self, run: &p::RunId) -> bool {
        self.streams.contains_key(run)
    }

    pub fn contains_event(&self, event: &p::Event) -> bool {
        self.streams
            .get(&event.run_id)
            .is_some_and(|head| event.stream_seq <= *head)
    }

    fn validate(&self) -> p::Result<()> {
        if self.schema_version != p::SchemaVersion(1)
            || self.generation == 0
            || (self.generation == 1) != self.previous_digest.is_none()
            || self
                .streams
                .iter()
                .any(|(run, seq)| run.0.is_empty() || *seq == 0)
        {
            return Err(invalid());
        }
        for digest in [
            &self.owner_digest,
            &self.store_identity,
            &self.source_database_digest,
            &self.plan_digest,
        ]
        .into_iter()
        .chain(self.previous_digest.iter())
        {
            p::sha256_digest_bytes(digest)?;
        }
        Ok(())
    }
}

impl SqliteEventStore {
    pub fn restored_execution_fence(&self) -> p::Result<Option<RestoredExecutionFence>> {
        let connection = self.core_connection()?;
        read(&connection)
    }

    pub fn require_current_execution(&self, run: &p::RunId) -> p::Result<()> {
        if self
            .restored_execution_fence()?
            .is_some_and(|fence| fence.contains(run))
        {
            return Err(p::Error(
                "restored historical execution is quarantined; create a new owner plan".into(),
            ));
        }
        Ok(())
    }

    pub fn quarantined_intentions(&self) -> p::Result<BTreeSet<p::IntentionId>> {
        use crate::EventStore;
        let Some(fence) = self.restored_execution_fence()? else {
            return Ok(BTreeSet::new());
        };
        let mut ids = BTreeSet::new();
        for run in fence.streams.keys() {
            for event in self.read_run(run.clone()) {
                let event = event?;
                if fence.contains_event(&event) {
                    if let p::EventPayload::ProspectiveIntentionCreated(created) = event.payload {
                        ids.insert(created.intention_id);
                    }
                }
            }
        }
        Ok(ids)
    }
}

pub(crate) fn read(connection: &Connection) -> p::Result<Option<RestoredExecutionFence>> {
    // Read-only old-schema inspection is allowed before its explicit upgrade.
    let exists: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='brain_restore_fences')",
            [], |r| r.get(0),
        )
        .map_err(|_| invalid())?;
    if !exists {
        return Ok(None);
    }
    let mut query = connection
        .prepare("SELECT generation,payload,digest,length(payload) FROM brain_restore_fences ORDER BY generation")
        .map_err(|_| invalid())?;
    let mut rows = query.query([]).map_err(|_| invalid())?;
    let mut latest: Option<RestoredExecutionFence> = None;
    while let Some(row) = rows.next().map_err(|_| invalid())? {
        let size: i64 = row.get(3).map_err(|_| invalid())?;
        if !(0..=MAX_FENCE_BYTES as i64).contains(&size) {
            return Err(invalid());
        }
        let generation =
            u64::try_from(row.get::<_, i64>(0).map_err(|_| invalid())?).map_err(|_| invalid())?;
        let bytes: Vec<u8> = row.get(1).map_err(|_| invalid())?;
        let digest: String = row.get(2).map_err(|_| invalid())?;
        let fence: RestoredExecutionFence =
            serde_json::from_slice(&bytes).map_err(|_| invalid())?;
        fence.validate()?;
        let previous_digest = latest
            .as_ref()
            .map(RestoredExecutionFence::digest)
            .transpose()?;
        if generation != fence.generation
            || fence.generation
                != latest
                    .as_ref()
                    .map_or(1, |old| old.generation.saturating_add(1))
            || fence.previous_digest != previous_digest
            || fence.digest()?.0 != digest
            || latest.as_ref().is_some_and(|old| {
                old.owner_digest != fence.owner_digest
                    || old.store_identity != fence.store_identity
                    || old
                        .streams
                        .iter()
                        .any(|(run, head)| fence.streams.get(run).is_none_or(|new| new < head))
            })
        {
            return Err(invalid());
        }
        latest = Some(fence);
    }
    if let Some(fence) = &latest {
        let identity: (String, String) = connection
            .query_row(
                "SELECT owner_digest,store_identity FROM brain_authority",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(|_| invalid())?;
        if identity != (fence.owner_digest.0.clone(), fence.store_identity.0.clone()) {
            return Err(invalid());
        }
        let current = stream_heads(connection)?;
        if fence
            .streams
            .iter()
            .any(|(run, head)| current.get(run).is_none_or(|value| value < head))
        {
            return Err(invalid());
        }
    }
    Ok(latest)
}

/// Called only on the newly copied target while its exclusive OS fence is held.
/// It never edits events, leases or the source, and never claims authority.
pub(crate) fn seal_target(
    path: &Path,
    source: &OfflineBrainIdentity,
    plan: &p::SchemaDigest,
) -> p::Result<p::SchemaDigest> {
    seal_snapshot(
        path,
        &source.owner_digest,
        &source.store_identity,
        &source.database_digest,
        plan,
    )
}

pub(crate) fn seal_snapshot(
    path: &Path,
    owner: &p::SchemaDigest,
    store: &p::SchemaDigest,
    source_digest: &p::SchemaDigest,
    plan: &p::SchemaDigest,
) -> p::Result<p::SchemaDigest> {
    let mut connection =
        Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE)
            .map_err(|_| invalid())?;
    connection
        .pragma_update(None, "synchronous", "FULL")
        .map_err(|_| invalid())?;
    let tx = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|_| invalid())?;
    let identity: (String, String) = tx
        .query_row(
            "SELECT owner_digest,store_identity FROM brain_authority",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|_| invalid())?;
    if identity != (owner.0.clone(), store.0.clone()) {
        return Err(invalid());
    }
    let previous = read(&tx)?;
    tx.execute_batch(SCHEMA).map_err(|_| invalid())?;
    let streams = stream_heads(&tx)?;
    let fence = RestoredExecutionFence {
        schema_version: p::SchemaVersion(1),
        generation: previous.as_ref().map_or(Ok(1), |old| {
            old.generation.checked_add(1).ok_or_else(invalid)
        })?,
        owner_digest: owner.clone(),
        store_identity: store.clone(),
        source_database_digest: source_digest.clone(),
        plan_digest: plan.clone(),
        previous_digest: previous
            .as_ref()
            .map(RestoredExecutionFence::digest)
            .transpose()?,
        streams,
    };
    fence.validate()?;
    let bytes = serde_json::to_vec(&fence).map_err(|_| invalid())?;
    if bytes.len() > MAX_FENCE_BYTES {
        return Err(invalid());
    }
    let digest = fence.digest()?;
    tx.execute(
        "INSERT INTO brain_restore_fences(generation,payload,digest) VALUES(?1,?2,?3)",
        params![
            i64::try_from(fence.generation).map_err(|_| invalid())?,
            bytes,
            digest.0
        ],
    )
    .map_err(|_| invalid())?;
    tx.pragma_update(None, "user_version", 10)
        .map_err(|_| invalid())?;
    tx.commit().map_err(|_| invalid())?;
    connection
        .execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")
        .map_err(|_| invalid())?;
    connection.close().map_err(|_| invalid())?;
    Ok(digest)
}

/// Last guard before a historical run could regain model/tool authority.
pub(crate) fn enforce_event(connection: &Connection, event: &p::Event) -> p::Result<()> {
    let execution = matches!(
        event.kind,
        p::EventKind::ModelCallStarted | p::EventKind::ActionStarted | p::EventKind::RunResumed
    );
    let fired = match &event.payload {
        p::EventPayload::ProspectiveIntentionResolved(resolved)
            if resolved.outcome == p::IntentionOutcome::Fired =>
        {
            Some(&resolved.intention_id)
        }
        _ => None,
    };
    if !execution && fired.is_none() {
        return Ok(());
    }
    if let Some(fence) = read(connection)? {
        if (execution && fence.contains(&event.run_id))
            || fired
                .map(|id| contains_intention(connection, &fence, id))
                .transpose()?
                .unwrap_or(false)
        {
            return Err(p::Error(
                "restored historical execution is quarantined; create a new owner plan".into(),
            ));
        }
    }
    Ok(())
}

fn contains_intention(
    c: &Connection,
    fence: &RestoredExecutionFence,
    id: &p::IntentionId,
) -> p::Result<bool> {
    let mut statement = c
        .prepare("SELECT event_id,run_id,stream_seq,payload FROM events WHERE kind=?1")
        .map_err(|_| invalid())?;
    let mut rows = statement
        .query(params![p::EventKind::ProspectiveIntentionCreated.as_str()])
        .map_err(|_| invalid())?;
    while let Some(row) = rows.next().map_err(|_| invalid())? {
        let run = p::RunId(row.get(1).map_err(|_| invalid())?);
        let seq =
            u64::try_from(row.get::<_, i64>(2).map_err(|_| invalid())?).map_err(|_| invalid())?;
        if fence.streams.get(&run).is_none_or(|head| seq > *head) {
            continue;
        }
        let key: String = row.get(0).map_err(|_| invalid())?;
        let bytes: Vec<u8> = row.get(3).map_err(|_| invalid())?;
        let bytes = crate::at_rest::open_bound_blob(c, &bytes, "events", &key)?;
        let payload: p::EventPayload = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
        if matches!(payload, p::EventPayload::ProspectiveIntentionCreated(p) if &p.intention_id == id)
        {
            return Ok(true);
        }
    }
    Ok(false)
}

pub(crate) fn stream_heads(c: &Connection) -> p::Result<BTreeMap<p::RunId, u64>> {
    let mut statement = c
        .prepare("SELECT run_id,MAX(stream_seq) FROM events GROUP BY run_id")
        .map_err(|_| invalid())?;
    let values = statement
        .query_map([], |r| Ok((p::RunId(r.get(0)?), r.get::<_, i64>(1)?)))
        .map_err(|_| invalid())?;
    values
        .map(|value| {
            let (run, head) = value.map_err(|_| invalid())?;
            Ok((run, u64::try_from(head).map_err(|_| invalid())?))
        })
        .collect()
}
