use std::{
    path::Path,
    sync::{Arc, Mutex},
};

use forme_protocol as p;
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};

use crate::ReplicaProjectionStore;

const REPLICA_SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS replica_batches (
    batch_ref      TEXT PRIMARY KEY,
    peer_ref       TEXT NOT NULL,
    aggregate      TEXT NOT NULL,
    from_seq       INTEGER NOT NULL,
    to_seq         INTEGER NOT NULL,
    authority_epoch INTEGER NOT NULL,
    semantic_bytes BLOB NOT NULL
);
CREATE TABLE IF NOT EXISTS replica_events (
    peer_ref       TEXT NOT NULL,
    aggregate      TEXT NOT NULL,
    stream_seq     INTEGER NOT NULL,
    envelope       BLOB NOT NULL,
    PRIMARY KEY(peer_ref, aggregate, stream_seq)
);
CREATE TABLE IF NOT EXISTS replica_cursors (
    peer_ref       TEXT NOT NULL,
    aggregate      TEXT NOT NULL,
    stream_seq     INTEGER NOT NULL,
    authority_epoch INTEGER NOT NULL,
    PRIMARY KEY(peer_ref, aggregate)
);
"#;

/// A physically separate, read-only projection database.  This type does not
/// implement `EventStore`, which keeps authority append unavailable by type.
#[derive(Clone)]
pub struct ReplicaSqliteStore {
    connection: Arc<Mutex<Connection>>,
    peer: p::FederatedPeerRef,
    grant: p::FederatedPeerGrantRef,
    epoch: p::AuthorityEpoch,
    scope: p::ReplicaScope,
}

impl ReplicaSqliteStore {
    pub fn open(
        path: impl AsRef<Path>,
        peer: p::FederatedPeerRef,
        grant: p::FederatedPeerGrantRef,
        epoch: p::AuthorityEpoch,
        scope: p::ReplicaScope,
    ) -> p::Result<Self> {
        let connection = Connection::open(path)
            .map_err(|error| p::Error(format!("failed to open replica database: {error}")))?;
        Self::from_connection(connection, peer, grant, epoch, scope)
    }

    pub fn open_in_memory(
        peer: p::FederatedPeerRef,
        grant: p::FederatedPeerGrantRef,
        epoch: p::AuthorityEpoch,
        scope: p::ReplicaScope,
    ) -> p::Result<Self> {
        let connection = Connection::open_in_memory()
            .map_err(|error| p::Error(format!("failed to open replica database: {error}")))?;
        Self::from_connection(connection, peer, grant, epoch, scope)
    }

    fn from_connection(
        connection: Connection,
        peer: p::FederatedPeerRef,
        grant: p::FederatedPeerGrantRef,
        epoch: p::AuthorityEpoch,
        scope: p::ReplicaScope,
    ) -> p::Result<Self> {
        if peer.0.trim().is_empty() || grant.0.trim().is_empty() || epoch.0 == 0 {
            return Err(p::Error("replica identity binding is incomplete".into()));
        }
        scope.validate()?;
        connection
            .execute_batch(REPLICA_SCHEMA)
            .map_err(|error| p::Error(format!("failed to initialize replica database: {error}")))?;
        Ok(Self {
            connection: Arc::new(Mutex::new(connection)),
            peer,
            grant,
            epoch,
            scope,
        })
    }

    fn current_cursor(
        &self,
        connection: &Connection,
        aggregate: &p::RunId,
    ) -> p::Result<p::ReplicationCursor> {
        let stored = connection
            .query_row(
                "SELECT stream_seq, authority_epoch FROM replica_cursors
                 WHERE peer_ref = ?1 AND aggregate = ?2",
                params![&self.peer.0, &aggregate.0],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
            )
            .optional()
            .map_err(|error| p::Error(format!("failed to read replica cursor: {error}")))?;
        let (stream_seq, epoch) = stored.unwrap_or((0, self.epoch.0 as i64));
        Ok(p::ReplicationCursor {
            schema_version: p::M4_SCHEMA_VERSION,
            peer: self.peer.clone(),
            aggregate: aggregate.clone(),
            stream_seq: u64::try_from(stream_seq)
                .map_err(|_| p::Error("stored replica cursor is invalid".into()))?,
            authority_epoch: p::AuthorityEpoch(
                u64::try_from(epoch)
                    .map_err(|_| p::Error("stored replica epoch is invalid".into()))?,
            ),
        })
    }

    fn projection_digest(
        &self,
        connection: &Connection,
        aggregate: Option<&p::RunId>,
    ) -> p::Result<p::SchemaDigest> {
        let mut bytes = Vec::<Vec<u8>>::new();
        if let Some(aggregate) = aggregate {
            let mut statement = connection
                .prepare(
                    "SELECT envelope FROM replica_events
                     WHERE peer_ref = ?1 AND aggregate = ?2 ORDER BY stream_seq",
                )
                .map_err(|error| p::Error(format!("failed to prepare replica digest: {error}")))?;
            let rows = statement
                .query_map(params![&self.peer.0, &aggregate.0], |row| row.get(0))
                .map_err(|error| {
                    p::Error(format!("failed to read replica digest rows: {error}"))
                })?;
            bytes.extend(
                rows.collect::<rusqlite::Result<Vec<_>>>()
                    .map_err(|error| {
                        p::Error(format!(
                            "failed to materialize replica digest rows: {error}"
                        ))
                    })?,
            );
        } else {
            let mut statement = connection
                .prepare(
                    "SELECT envelope FROM replica_events
                     WHERE peer_ref = ?1 ORDER BY aggregate, stream_seq",
                )
                .map_err(|error| p::Error(format!("failed to prepare replica digest: {error}")))?;
            let rows = statement
                .query_map(params![&self.peer.0], |row| row.get(0))
                .map_err(|error| {
                    p::Error(format!("failed to read replica digest rows: {error}"))
                })?;
            bytes.extend(
                rows.collect::<rusqlite::Result<Vec<_>>>()
                    .map_err(|error| {
                        p::Error(format!(
                            "failed to materialize replica digest rows: {error}"
                        ))
                    })?,
            );
        }
        p::canonical_digest(&bytes)
    }
}

impl ReplicaProjectionStore for ReplicaSqliteStore {
    fn cursor(
        &self,
        peer: &p::FederatedPeerRef,
        aggregate: &p::RunId,
    ) -> p::Result<p::ReplicationCursor> {
        if peer != &self.peer || aggregate.0.trim().is_empty() {
            return Err(p::Error(
                "replica cursor request is outside its binding".into(),
            ));
        }
        let connection = self
            .connection
            .lock()
            .map_err(|_| p::Error("replica database is unavailable".into()))?;
        self.current_cursor(&connection, aggregate)
    }

    fn apply(
        &self,
        batch: p::ReplicationBatch,
        expected: p::ReplicationCursor,
    ) -> p::Result<p::ReplicaApplyReport> {
        batch.validate()?;
        expected.validate()?;
        if batch.peer != self.peer
            || batch.peer_grant != self.grant
            || batch.from.authority_epoch != self.epoch
            || batch.to.authority_epoch != self.epoch
        {
            return Err(p::Error(
                "replication batch is outside the replica binding".into(),
            ));
        }
        let mut normalized = batch.clone();
        normalized.refresh_digest()?;
        if normalized.content_digest != batch.content_digest {
            return Err(p::Error(
                "replication batch digest does not match its contents".into(),
            ));
        }
        validate_replica_events(&batch)?;
        let semantic_bytes = serde_json::to_vec(&batch)
            .map_err(|error| p::Error(format!("failed to encode replication batch: {error}")))?;

        let mut connection = self
            .connection
            .lock()
            .map_err(|_| p::Error("replica database is unavailable".into()))?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| p::Error(format!("failed to begin replica apply: {error}")))?;

        if let Some(stored) = transaction
            .query_row(
                "SELECT semantic_bytes FROM replica_batches WHERE batch_ref = ?1",
                params![&batch.batch.0],
                |row| row.get::<_, Vec<u8>>(0),
            )
            .optional()
            .map_err(|error| p::Error(format!("failed to inspect replica batch: {error}")))?
        {
            if stored != semantic_bytes {
                return Err(p::Error(
                    "replication batch id was reused with new semantics".into(),
                ));
            }
            let cursor = self.current_cursor(&transaction, &batch.aggregate)?;
            let projection_digest = self.projection_digest(&transaction, Some(&batch.aggregate))?;
            transaction
                .commit()
                .map_err(|error| p::Error(format!("failed to finish duplicate apply: {error}")))?;
            return Ok(p::ReplicaApplyReport {
                schema_version: p::M4_SCHEMA_VERSION,
                batch: batch.batch,
                status: p::ReplicaApplyStatus::Duplicate,
                cursor,
                projection_digest,
                applied_events: Vec::new(),
            });
        }

        let actual = self.current_cursor(&transaction, &batch.aggregate)?;
        if batch.from != expected || actual != expected {
            return Err(p::Error("replica cursor compare-and-swap conflict".into()));
        }
        for event in &batch.events {
            let envelope = serde_json::to_vec(event)
                .map_err(|error| p::Error(format!("failed to encode replica event: {error}")))?;
            transaction
                .execute(
                    "INSERT INTO replica_events(peer_ref, aggregate, stream_seq, envelope)
                     VALUES (?1, ?2, ?3, ?4)",
                    params![
                        &self.peer.0,
                        &batch.aggregate.0,
                        i64::try_from(event.source_stream_seq)
                            .map_err(|_| p::Error("replica sequence is too large".into()))?,
                        envelope,
                    ],
                )
                .map_err(|error| p::Error(format!("failed to append replica event: {error}")))?;
        }
        transaction
            .execute(
                "INSERT INTO replica_batches(
                    batch_ref, peer_ref, aggregate, from_seq, to_seq,
                    authority_epoch, semantic_bytes
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    &batch.batch.0,
                    &self.peer.0,
                    &batch.aggregate.0,
                    i64::try_from(batch.from.stream_seq)
                        .map_err(|_| p::Error("replica cursor is too large".into()))?,
                    i64::try_from(batch.to.stream_seq)
                        .map_err(|_| p::Error("replica cursor is too large".into()))?,
                    i64::try_from(self.epoch.0)
                        .map_err(|_| p::Error("replica epoch is too large".into()))?,
                    semantic_bytes,
                ],
            )
            .map_err(|error| p::Error(format!("failed to record replica batch: {error}")))?;
        transaction
            .execute(
                "INSERT INTO replica_cursors(peer_ref, aggregate, stream_seq, authority_epoch)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(peer_ref, aggregate) DO UPDATE SET
                    stream_seq = excluded.stream_seq,
                    authority_epoch = excluded.authority_epoch",
                params![
                    &self.peer.0,
                    &batch.aggregate.0,
                    i64::try_from(batch.to.stream_seq)
                        .map_err(|_| p::Error("replica cursor is too large".into()))?,
                    i64::try_from(self.epoch.0)
                        .map_err(|_| p::Error("replica epoch is too large".into()))?,
                ],
            )
            .map_err(|error| p::Error(format!("failed to advance replica cursor: {error}")))?;

        let projection_digest = self.projection_digest(&transaction, Some(&batch.aggregate))?;
        let applied_events = batch
            .events
            .iter()
            .map(|event| event.event_id.clone())
            .collect();
        let cursor = batch.to.clone();
        transaction
            .commit()
            .map_err(|error| p::Error(format!("failed to commit replica apply: {error}")))?;
        Ok(p::ReplicaApplyReport {
            schema_version: p::M4_SCHEMA_VERSION,
            batch: batch.batch,
            status: p::ReplicaApplyStatus::Applied,
            cursor,
            projection_digest,
            applied_events,
        })
    }

    fn rebuild(&self, scope: p::ReplicaScope) -> p::Result<p::ReplicaProjectionDigestRef> {
        scope.validate()?;
        if scope != self.scope {
            return Err(p::Error(
                "replica rebuild scope does not match its grant".into(),
            ));
        }
        let connection = self
            .connection
            .lock()
            .map_err(|_| p::Error("replica database is unavailable".into()))?;
        Ok(p::ReplicaProjectionDigestRef(
            self.projection_digest(&connection, None)?.0,
        ))
    }
}

fn validate_replica_events(batch: &p::ReplicationBatch) -> p::Result<()> {
    const AUTHORITY_ONLY: [p::EventKind; 4] = [
        p::EventKind::FederatedPeerRegistered,
        p::EventKind::FederatedPeerRevoked,
        p::EventKind::RemoteExecutionLeaseChanged,
        p::EventKind::ReplicationCheckpointAdvanced,
    ];
    const SENSITIVE: [p::EventKind; 4] = [
        p::EventKind::RunAccepted,
        p::EventKind::ModelCallDelta,
        p::EventKind::ToolCallProposed,
        p::EventKind::ActionOutputDelta,
    ];
    for event in &batch.events {
        match &event.payload {
            p::SyncTransferPayload::Full(payload)
                if payload.kind() == event.kind
                    && !AUTHORITY_ONLY.contains(&event.kind)
                    && !SENSITIVE.contains(&event.kind) => {}
            p::SyncTransferPayload::Redacted {
                kind,
                reason,
                digest,
            } if *kind == event.kind
                && !reason.0.trim().is_empty()
                && !digest.0.trim().is_empty() => {}
            _ => {
                return Err(p::Error(
                    "replica event payload is unsafe or does not match its kind".into(),
                ));
            }
        }
        let encoded = serde_json::to_string(event)
            .map_err(|error| p::Error(format!("failed to inspect replica event: {error}")))?
            .to_ascii_lowercase();
        if encoded.contains("secretref")
            || encoded.contains("credentialref")
            || encoded.contains("private endpoint")
        {
            return Err(p::Error(
                "replica event contains restricted material".into(),
            ));
        }
    }
    Ok(())
}
