//! Atomic orchestration ledger projection. Event append remains the only writer.
use forme_protocol as p;
use rusqlite::{params, Connection, OptionalExtension, Transaction};

pub(crate) const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS orchestration_state (
    stream TEXT PRIMARY KEY, revision INTEGER NOT NULL, payload_json TEXT NOT NULL, digest TEXT NOT NULL);";

pub(crate) fn read(
    connection: &Connection,
    stream: &p::RunId,
) -> p::Result<p::OrchestrationLedger> {
    let row = connection
        .query_row(
            "SELECT revision,payload_json,digest FROM orchestration_state WHERE stream=?1",
            params![stream.0],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .optional()
        .map_err(|_| p::Error("orchestration ledger read failed".into()))?;
    let Some((revision, json, digest)) = row else {
        return Ok(p::OrchestrationLedger::default());
    };
    let json = crate::at_rest::open_bound_blob(
        connection,
        json.as_bytes(),
        "orchestration_state",
        &stream.0,
    )?;
    let ledger: p::OrchestrationLedger = serde_json::from_slice(&json)
        .map_err(|_| p::Error("orchestration projection is corrupt".into()))?;
    if Some(ledger.revision) != u64::try_from(revision).ok()
        || p::canonical_digest(&ledger)?.0 != digest
    {
        return Err(p::Error(
            "orchestration projection integrity failure".into(),
        ));
    }
    Ok(ledger)
}

pub(crate) fn validate(tx: &Transaction<'_>, event: &p::Event) -> p::Result<()> {
    let p::EventPayload::DecisionTraceRecorded(payload) = &event.payload else {
        return Ok(());
    };
    let Some(fact) = &payload.orchestration else {
        return Ok(());
    };
    let stream: Option<String> = tx
        .query_row(
            "SELECT aggregate_run_id FROM brain_aggregates WHERE kind='charter'",
            [],
            |r| r.get(0),
        )
        .optional()
        .map_err(|_| p::Error("orchestration authority lookup failed".into()))?;
    if stream.as_deref() != Some(event.run_id.0.as_str())
        || event.provenance.trust_tier == p::TrustTier::Untrusted
    {
        return Err(p::Error(
            "orchestration facts require the owner authority stream".into(),
        ));
    }
    if matches!(
        fact.operation,
        p::OrchestrationOperation::ProjectDeclared(_)
            | p::OrchestrationOperation::ProjectRevised { .. }
            | p::OrchestrationOperation::ProjectSupervisionChanged { .. }
    ) && (event.provenance.actor != p::Actor::Owner
        || event.provenance.trust_tier != p::TrustTier::OwnerInput)
    {
        return Err(p::Error(
            "project declaration requires authenticated owner provenance".into(),
        ));
    }
    let mut ledger = read(tx, &event.run_id)?;
    let charter = crate::charter::read(tx, &fact.workspace)?
        .ok_or_else(|| p::Error("project orchestration requires a charter".into()))?;
    let rules = p::CharterRules::parse(&charter)?;
    match &fact.operation {
        p::OrchestrationOperation::ProjectDeclared(project)
        | p::OrchestrationOperation::ProjectRevised { project, .. } => {
            if project
                .capabilities
                .iter()
                .any(|c| !rules.permits_capability(c))
                || rules
                    .scope
                    .as_ref()
                    .is_some_and(|s| !p::charter_scope_contains(s, &project.scope))
            {
                return Err(p::Error("project resources exceed its charter".into()));
            }
        }
        p::OrchestrationOperation::RouteDeclared(route) => {
            if route.charter_digest != charter.digest
                || rules.max_nodes.is_some_and(|n| route.nodes.len() > n)
                || route
                    .nodes
                    .iter()
                    .chain(std::iter::once(&route.main))
                    .any(|n| {
                        n.capabilities.iter().any(|c| !rules.permits_capability(c))
                            || rules
                                .scope
                                .as_ref()
                                .is_some_and(|s| !p::charter_scope_contains(s, &n.scope))
                    })
                || (rules.require_review && !route.checkpoint_required)
            {
                return Err(p::Error("route selection violates current charter".into()));
            }
        }
        p::OrchestrationOperation::NodeStarted { route, node, .. } => {
            let spec = ledger
                .routes
                .get(route)
                .and_then(|r| r.node_spec(node))
                .ok_or_else(|| p::Error("unknown route node".into()))?;
            if spec
                .capabilities
                .iter()
                .any(|c| !rules.permits_capability(c))
                || rules
                    .scope
                    .as_ref()
                    .is_some_and(|s| !p::charter_scope_contains(s, &spec.scope))
            {
                return Err(p::Error("updated charter blocks child claim".into()));
            }
        }
        _ => {}
    }
    ledger.apply(fact)
}

pub(crate) fn fold(tx: &Transaction<'_>, event: &p::Event) -> p::Result<()> {
    let p::EventPayload::DecisionTraceRecorded(payload) = &event.payload else {
        return Ok(());
    };
    let Some(fact) = &payload.orchestration else {
        return Ok(());
    };
    let mut ledger = read(tx, &event.run_id)?;
    ledger.apply(fact)?;
    let encoded = serde_json::to_vec(&ledger)
        .map_err(|_| p::Error("orchestration ledger encoding failed".into()))?;
    let encoded = String::from_utf8(crate::at_rest::seal_blob(
        tx,
        &encoded,
        None,
        "orchestration_state",
        &event.run_id.0,
        &event.run_id.0,
    )?)
    .map_err(|_| p::Error("orchestration protected encoding failed".into()))?;
    tx.execute("INSERT INTO orchestration_state(stream,revision,payload_json,digest) VALUES(?1,?2,?3,?4)
        ON CONFLICT(stream) DO UPDATE SET revision=excluded.revision,payload_json=excluded.payload_json,digest=excluded.digest",
        params![event.run_id.0, i64::try_from(ledger.revision).map_err(|_| p::Error("orchestration revision exhausted".into()))?, encoded, p::canonical_digest(&ledger)?.0])
        .map_err(|_| p::Error("orchestration ledger commit failed".into()))?;
    Ok(())
}
