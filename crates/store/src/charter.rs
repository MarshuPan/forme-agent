//! Charter projection shares the event append transaction; no independent writer.
use forme_protocol as p;
use rusqlite::{params, Connection, OptionalExtension, Transaction};

pub const CHARTER_SCHEMA: &str = "CREATE TABLE IF NOT EXISTS workspace_charter_state (
    workspace TEXT PRIMARY KEY, version INTEGER NOT NULL, event_id TEXT NOT NULL,
    payload_json TEXT NOT NULL, digest TEXT NOT NULL);";

pub(crate) fn read(
    connection: &Connection,
    workspace: &p::WorkspaceRef,
) -> p::Result<Option<p::WorkspaceCharterRecord>> {
    let row = connection
        .query_row(
            "SELECT payload_json, version, digest FROM workspace_charter_state WHERE workspace=?1",
            params![workspace.0],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .optional()
        .map_err(|_| p::Error("charter projection read failed".into()))?;
    row.map(|(json, version, digest)| {
        let record: p::WorkspaceCharterRecord = serde_json::from_str(&json)
            .map_err(|_| p::Error("charter projection is corrupt".into()))?;
        record.validate()?;
        if Some(record.version) != u64::try_from(version).ok()
            || record.digest.0 != digest
            || record.workspace != *workspace
        {
            return Err(p::Error("charter projection binding is corrupt".into()));
        }
        Ok(record)
    })
    .transpose()
}

pub(crate) fn validate(transaction: &Transaction<'_>, event: &p::Event) -> p::Result<()> {
    let p::EventPayload::WorkspaceCharterChanged(payload) = &event.payload else {
        return Ok(());
    };
    payload.validate()?;
    p::CharterRules::parse(&payload.charter)?;
    if event.provenance.actor != p::Actor::Owner
        || event.provenance.trust_tier != p::TrustTier::OwnerInput
    {
        return Err(p::Error(
            "charter mutation requires authenticated owner provenance".into(),
        ));
    }
    let stream: Option<String> = transaction
        .query_row(
            "SELECT aggregate_run_id FROM brain_aggregates WHERE kind='charter'",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(|_| p::Error("charter authority lookup failed".into()))?;
    if stream.as_deref() != Some(event.run_id.0.as_str()) {
        return Err(p::Error(
            "charter mutation names a different owner aggregate".into(),
        ));
    }
    let current = read(transaction, &payload.charter.workspace)?.map_or(0, |r| r.version);
    if current != payload.expected_version {
        return Err(p::Error("charter expected version conflict".into()));
    }
    Ok(())
}

pub(crate) fn fold(transaction: &Transaction<'_>, event: &p::Event) -> p::Result<()> {
    let p::EventPayload::WorkspaceCharterChanged(payload) = &event.payload else {
        return Ok(());
    };
    let record = &payload.charter;
    let current = read(transaction, &record.workspace)?.map_or(0, |r| r.version);
    if current != payload.expected_version {
        return Err(p::Error("charter replay version discontinuity".into()));
    }
    transaction.execute("INSERT INTO workspace_charter_state(workspace,version,event_id,payload_json,digest) VALUES (?1,?2,?3,?4,?5)
        ON CONFLICT(workspace) DO UPDATE SET version=excluded.version,event_id=excluded.event_id,payload_json=excluded.payload_json,digest=excluded.digest",
        params![record.workspace.0, i64::try_from(record.version).map_err(|_| p::Error("charter version exhausted".into()))?, event.event_id.0, serde_json::to_string(record).map_err(|_| p::Error("charter encoding failed".into()))?, record.digest.0])
        .map_err(|_| p::Error("charter projection commit failed".into()))?;
    Ok(())
}
