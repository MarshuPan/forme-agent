//! Local owner correction: source, invalidation and candidate commit together.
use super::*;
use crate::ProtectedContentRecord;
use std::collections::BTreeSet;

impl SqliteEventStore {
    /// Authority-local CAS; not a peer/import path. Every input has already
    /// passed the owner's semantic checks in the production Harness. Sources
    /// and all dependent candidate facts roll back if any append fails.
    pub fn append_owner_correction(
        &self,
        expected: Vec<p::AggregateVersion>,
        events: Vec<p::Event>,
        sources: Vec<ProtectedContentRecord>,
        dependent_events: Vec<p::EventId>,
    ) -> p::Result<Vec<p::EventId>> {
        self.require_authority_writer()?;
        validate(&expected, &events, &sources, &dependent_events)?;
        let protection = self
            .content_protection()?
            .ok_or_else(|| p::Error("owner correction content protection is unavailable".into()))?;
        let mut plaintext = Vec::new();
        for source in &sources {
            source.validate()?;
            if source.owner_digest != *protection.owner_digest()
                || source.envelope.ciphertext.len() > 32_768
                || source.scope != sources[0].scope
                || source.subject_digest != sources[0].subject_digest
            {
                return Err(p::Error(
                    "owner correction source binding is invalid".into(),
                ));
            }
            plaintext.push(
                protection
                    .content_service()
                    .decrypt(&source.envelope.binding(), &source.envelope)?,
            );
        }
        let audit_facts: Vec<_> = events
            .iter()
            .filter(|event| !dependent_events.contains(&event.event_id))
            .map(|event| (event.event_id.clone(), event.provenance.caused_by.clone()))
            .collect();
        let prepared = events
            .into_iter()
            .map(|e| self.prepare_event(e))
            .collect::<p::Result<Vec<_>>>()?;
        let _registration = if self.core.fts_enabled {
            Some(
                self.core
                    .upcaster_registration
                    .lock()
                    .map_err(|_| p::Error("upcaster registration lock is poisoned".into()))?,
            )
        } else {
            None
        };
        let upcasters = if self.core.fts_enabled {
            Some(
                self.core
                    .upcasters
                    .read()
                    .map_err(|_| p::Error("upcaster graph lock is poisoned".into()))?
                    .identity_bytes(),
            )
        } else {
            None
        };
        let schema = self
            .core
            .fts_enabled
            .then(|| schema_registry_identity(&self.core.current_schema));
        let mut connection = self
            .core
            .connection
            .lock()
            .map_err(|_| p::Error("event database lock is poisoned".into()))?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| p::Error("owner correction transaction is unavailable".into()))?;
        self.require_authority_writer_in_transaction(&transaction)?;
        for version in &expected {
            if aggregate_version_in_transaction(&transaction, &version.aggregate)? != version.value
            {
                return Err(p::Error("owner correction source version is stale".into()));
            }
        }
        for (record, bytes) in sources.iter().zip(&plaintext) {
            super::protected_companion::insert_companion(&transaction, record, bytes, &protection)?;
        }
        let mut ids = Vec::new();
        for event in prepared {
            let outcome = append_prepared(
                &transaction,
                event,
                self.core.fts_enabled,
                schema.as_deref(),
                upcasters.as_deref(),
            )?;
            if !matches!(outcome.disposition, AppendDisposition::Applied) {
                return Err(p::Error(
                    "owner correction requires its original receipt".into(),
                ));
            }
            ids.push(outcome.event_id);
        }
        // append_prepared conservatively links every fact to its aggregate
        // predecessor. These closed, content-free audit facts must survive
        // deletion of feedback in that predecessor. Remove only that implicit
        // edge; explicit references/causes and all candidate lineage remain.
        for (audit, cause) in audit_facts {
            transaction.execute(
                "DELETE FROM protected_dependencies WHERE object_ref=(SELECT object_ref FROM protected_rows
                   WHERE table_name='events' AND row_key=?1) AND source_object=(
                   SELECT r.object_ref FROM events previous JOIN protected_rows r ON r.table_name='events' AND r.row_key=previous.event_id
                   JOIN events current ON current.event_id=?1 WHERE previous.run_id=current.run_id AND previous.stream_seq<current.stream_seq
                   ORDER BY previous.stream_seq DESC LIMIT 1)
                   AND source_object NOT IN (SELECT object_ref FROM protected_rows WHERE table_name='events' AND row_key=?2)",
                params![audit.0, cause.as_ref().map(|id| id.0.as_str()).unwrap_or("")],
            ).map_err(|_|p::Error("owner correction audit lineage could not be bound".into()))?;
        }
        // The feedback text is a deletable source. Keep retraction/reevaluation
        // and the content-free receipt out of this set: erasure cannot revive
        // the old incorrect cognition. Candidate/FTS text must be cleaned.
        for event in &dependent_events {
            let changed = transaction
                .execute(
                    "INSERT OR IGNORE INTO protected_dependencies(object_ref,source_object)
                 SELECT object_ref,?1 FROM protected_rows WHERE table_name='events' AND row_key=?2",
                    params![sources[0].content_ref.0, event.0],
                )
                .map_err(|_| p::Error("owner correction source lineage is unavailable".into()))?;
            if changed == 0 {
                let recorded: i64 = transaction.query_row(
                    "SELECT COUNT(*) FROM protected_dependencies d JOIN protected_rows r
                     ON d.object_ref=r.object_ref WHERE r.table_name='events' AND r.row_key=?1 AND d.source_object=?2",
                    params![event.0, sources[0].content_ref.0], |row| row.get(0),
                ).map_err(|_| p::Error("owner correction lineage check failed".into()))?;
                if recorded != 1 {
                    return Err(p::Error(
                        "owner correction source lineage is missing".into(),
                    ));
                }
            }
        }
        transaction
            .commit()
            .map_err(|_| p::Error("owner correction commit outcome is unknown".into()))?;
        Ok(ids)
    }
}

fn validate(
    expected: &[p::AggregateVersion],
    events: &[p::Event],
    sources: &[ProtectedContentRecord],
    dependent: &[p::EventId],
) -> p::Result<()> {
    let invalid = || p::Error("owner correction batch is invalid or unbounded".into());
    if expected.is_empty()
        || expected.len() > 2
        || events.is_empty()
        || events.len() > 16
        || sources.len() != 2
        || dependent.is_empty()
        || dependent.len() > events.len()
    {
        return Err(invalid());
    }
    let streams: BTreeSet<_> = expected.iter().map(|v| &v.aggregate).collect();
    if streams.len() != expected.len()
        || streams != events.iter().map(|e| &e.run_id).collect()
        || sources[0].content_ref == sources[1].content_ref
    {
        return Err(invalid());
    }
    for version in expected {
        version.validate()?;
    }
    let ids: BTreeSet<_> = events.iter().map(|e| &e.event_id).collect();
    if ids.len() != events.len()
        || dependent.iter().collect::<BTreeSet<_>>().len() != dependent.len()
        || dependent.iter().any(|id| !ids.contains(id))
    {
        return Err(invalid());
    }
    let mut receipts = 0;
    for event in events {
        crate::domain_claim::validate_resolution_claim(event, false)?;
        reject_non_evolution_write_path(event)?;
        let allowed = match &event.payload {
            p::EventPayload::MemoryMaintenanceApplied(v) => {
                receipts += 1;
                v.deltas_ref.0.starts_with("workbench-correction-v1|")
            }
            p::EventPayload::MemoryNodeAppended(_)
            | p::EventPayload::ObservationRecorded(_)
            | p::EventPayload::ReflectionProduced(_)
            | p::EventPayload::CognitiveMapUpdateProposed(_)
            | p::EventPayload::CandidateCreated(_)
            | p::EventPayload::CandidateConflictDetected(_)
            | p::EventPayload::CandidateDowngraded(_)
            | p::EventPayload::RetractionEvent(_)
            | p::EventPayload::ReevaluationTaskCreated(_) => true,
            _ => false,
        };
        if !allowed {
            return Err(invalid());
        }
        let audit = matches!(
            &event.payload,
            p::EventPayload::RetractionEvent(_)
                | p::EventPayload::ReevaluationTaskCreated(_)
                | p::EventPayload::CandidateDowngraded(_)
                | p::EventPayload::MemoryMaintenanceApplied(_)
        );
        if audit == dependent.contains(&event.event_id) {
            return Err(invalid());
        }
    }
    if receipts != 1 {
        return Err(invalid());
    }
    Ok(())
}
