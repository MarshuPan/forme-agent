//! Empty session/transcript watermarks do not contain aggregate source text.
//! Prove both the current bytes and every historical event kind before
//! excluding these two caches from the conservative content dependency graph.
use crate::{Projection, SessionStateProjection, TranscriptProjection};
use forme_protocol as p;
use rusqlite::{params, Connection};
use std::collections::BTreeMap;

pub(crate) fn metadata_only(
    connection: &Connection,
    table: &str,
    key: &str,
    bytes: &[u8],
) -> p::Result<bool> {
    if !matches!(table, "session_state" | "transcript") {
        return Ok(false);
    }
    let value: serde_json::Value = serde_json::from_slice(bytes)
        .map_err(|_| p::Error("projection metadata could not be inspected".into()))?;
    let Some(sequence) = value.get("last_stream_seq").and_then(|v| v.as_u64()) else {
        return Ok(false);
    };
    let expected = if table == "session_state" {
        let mut empty = SessionStateProjection::empty();
        empty.run_id = Some(p::RunId(key.into()));
        empty.last_stream_seq = sequence;
        serde_json::to_value(empty)
    } else {
        let mut empty = TranscriptProjection::empty();
        empty.run_id = Some(p::RunId(key.into()));
        empty.last_stream_seq = sequence;
        serde_json::to_value(empty)
    }
    .map_err(|_| p::Error("projection metadata binding could not be encoded".into()))?;
    if value != expected {
        return Ok(false);
    }
    Ok(metadata_watermarks(connection, Some(key))?.contains_key(key))
}

// Retain the physical audit watermark even when every readable payload in a
// metadata-only aggregate has been erased. Inspect erased rows without opening
// their content, and never infer the same exemption for an unfamiliar schema.
pub(crate) fn metadata_watermarks(
    connection: &Connection,
    run: Option<&str>,
) -> p::Result<BTreeMap<String, u64>> {
    let mut statement = connection
        .prepare(
            "SELECT run_id,kind,schema_version,MAX(stream_seq) FROM events
             WHERE (?1 IS NULL OR run_id=?1) GROUP BY run_id,kind,schema_version",
        )
        .map_err(|_| p::Error("projection history could not be inspected".into()))?;
    let rows = statement
        .query_map(params![run], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
            ))
        })
        .map_err(|_| p::Error("projection history is unavailable".into()))?;
    let mut history = BTreeMap::<String, (u64, bool)>::new();
    for row in rows {
        let (run, kind, schema, sequence) =
            row.map_err(|_| p::Error("projection history is invalid".into()))?;
        let sequence = u64::try_from(sequence)
            .map_err(|_| p::Error("projection history sequence is invalid".into()))?;
        let inspected = history.entry(run).or_insert((0, true));
        inspected.0 = inspected.0.max(sequence);
        // Closed allowlist: unfamiliar events keep the conservative dependency.
        // None of these kinds changes either builtin projection's payload.
        inspected.1 &= schema == 1
            && matches!(
                kind.parse::<p::EventKind>()?,
                p::EventKind::CandidateCreated
                    | p::EventKind::ConfigDoctorReport
                    | p::EventKind::CandidateConflictDetected
                    | p::EventKind::CandidatePromoted
                    | p::EventKind::CandidateRejected
                    | p::EventKind::CandidateDowngraded
                    | p::EventKind::CandidateDecayed
                    | p::EventKind::RetractionEvent
                    | p::EventKind::RevocationEvent
                    | p::EventKind::ReevaluationTaskCreated
                    | p::EventKind::ObservationRecorded
                    | p::EventKind::OpportunityDetected
                    | p::EventKind::ValueGateEvaluated
                    | p::EventKind::CompetenceGateEvaluated
                    | p::EventKind::ImpulseRaised
                    | p::EventKind::ProactiveProposalEmitted
                    | p::EventKind::ProactiveProposalResolved
                    | p::EventKind::ProspectiveIntentionCreated
                    | p::EventKind::ProspectiveIntentionResolved
                    | p::EventKind::GoalFramed
                    | p::EventKind::ResourcePlanned
                    | p::EventKind::DoneContractSet
                    | p::EventKind::AutonomyEnvelopeSet
                    | p::EventKind::DecisionTraceRecorded
                    | p::EventKind::MemoryNodeAppended
                    | p::EventKind::MemoryEdgeAppended
                    | p::EventKind::MemoryMaintenanceApplied
                    | p::EventKind::ReflectionProduced
                    | p::EventKind::CognitiveMapUpdateProposed
                    | p::EventKind::UserAttributeCandidateCreated
                    | p::EventKind::ImportedHistoricalEvidenceRecorded
                    | p::EventKind::CapabilityEvidenceRecorded
                    | p::EventKind::EvolutionEvaluationRecorded
                    | p::EventKind::StrategyActivated
                    | p::EventKind::StrategyRolledBack
                    | p::EventKind::ApprovalRequested
                    | p::EventKind::ApprovalResolved
                    | p::EventKind::WorkspaceCharterChanged
                    | p::EventKind::DataLifecycleApplied
            );
    }
    Ok(history
        .into_iter()
        .filter_map(|(run, (head, safe))| safe.then_some((run, head)))
        .collect())
}

pub(crate) fn restore_watermark(
    run: &str,
    head: u64,
    session: &mut crate::SessionState,
    transcript: &mut crate::Transcript,
) {
    session.run_id = Some(p::RunId(run.into()));
    transcript.run_id = session.run_id.clone();
    session.last_stream_seq = head;
    transcript.last_stream_seq = head;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn history(kind: &str) -> Connection {
        let connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                "CREATE TABLE events(run_id TEXT,kind TEXT,schema_version INTEGER,stream_seq INTEGER)",
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO events VALUES('run:metadata',?1,1,1)",
                params![kind],
            )
            .unwrap();
        connection
    }
    fn empty_transcript() -> Vec<u8> {
        let mut state = TranscriptProjection::empty();
        state.run_id = Some(p::RunId("run:metadata".into()));
        state.last_stream_seq = 1;
        serde_json::to_vec(&state).unwrap()
    }

    #[test]
    fn empty_current_cache_cannot_exempt_a_historical_transcript_body() {
        let bytes = empty_transcript();
        let cognitive = history("MemoryNodeAppended");
        assert!(metadata_only(&cognitive, "transcript", "run:metadata", &bytes).unwrap());
        for kind in ["RunAccepted", "ModelCallDelta", "ActionOutputDelta"] {
            let connection = history(kind);
            assert!(!metadata_only(&connection, "transcript", "run:metadata", &bytes).unwrap());
        }
        let connection = history("UnknownFutureEvent");
        assert!(metadata_only(&connection, "transcript", "run:metadata", &bytes).is_err());
    }

    #[test]
    fn current_body_extra_fields_wrong_row_and_other_tables_keep_their_dependencies() {
        let connection = history("MemoryNodeAppended");
        let mut value: serde_json::Value = serde_json::from_slice(&empty_transcript()).unwrap();
        value["entries"] = serde_json::json!([{"text":"private source"}]);
        let bytes = serde_json::to_vec(&value).unwrap();
        assert!(!metadata_only(&connection, "transcript", "run:metadata", &bytes).unwrap());
        value["entries"] = serde_json::json!([]);
        value["extra"] = serde_json::json!("private source");
        let bytes = serde_json::to_vec(&value).unwrap();
        assert!(!metadata_only(&connection, "transcript", "run:metadata", &bytes).unwrap());
        let bytes = empty_transcript();
        assert!(!metadata_only(&connection, "transcript", "run:other", &bytes).unwrap());
        assert!(!metadata_only(&connection, "events", "run:metadata", &bytes).unwrap());
    }

    #[test]
    fn unfamiliar_historical_schema_and_missing_history_never_exempt_a_cache() {
        let connection = history("MemoryNodeAppended");
        connection
            .execute("UPDATE events SET schema_version=2", [])
            .unwrap();
        assert!(!metadata_only(
            &connection,
            "transcript",
            "run:metadata",
            &empty_transcript()
        )
        .unwrap());
        assert!(metadata_watermarks(&connection, None).unwrap().is_empty());
        connection.execute("DELETE FROM events", []).unwrap();
        assert!(!metadata_only(
            &connection,
            "transcript",
            "run:metadata",
            &empty_transcript()
        )
        .unwrap());
    }
}
