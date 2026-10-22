//! Exact review receipts: interrupted maintenance is queried, never replayed.
use super::*;

const RECEIPT_PREFIX: &str = "workbench-review-v1";

impl ProductionRuntime {
    pub fn review_workbench_candidate(
        &self,
        owner: &p::VerifiedPrincipal,
        command: p::WorkbenchReviewCommand,
    ) -> p::Result<p::WorkbenchCommandReceipt> {
        let _data = self.data_read_guard()?;
        self.require_brain_owner(owner)?;
        command.validate()?;
        let harness = self.harness();
        let key = p::canonical_digest(&(owner, &command.command))?;
        let binding = p::canonical_digest(&(owner, &command))?;
        let target = p::canonical_digest(&(&command.run, &command.candidate))?;
        let lock = harness.session_lock(&p::SessionId(format!("workbench-review:{}", key.0)))?;
        let _guard = lock
            .lock()
            .map_err(|_| p::Error("workbench review is unavailable".into()))?;
        let target_lock = harness.session_lock(&p::SessionId(format!(
            "workbench-review-target:{}",
            target.0
        )))?;
        let _target_guard = target_lock
            .lock()
            .map_err(|_| p::Error("workbench review is unavailable".into()))?;
        let events = self.owner_brain_events_at(None)?;
        if let Some(receipt) = previous(&events, &command.command, &key, &binding)? {
            return Ok(receipt);
        }
        if pending_target(&events, &target) {
            return Err(p::Error(
                "candidate has an unresolved review; inspect the original receipt".into(),
            ));
        }
        let run_events: Vec<_> = events
            .iter()
            .filter(|event| event.run_id == command.run)
            .cloned()
            .collect();
        let candidate = crate::candidate_snapshot(&command.candidate, &run_events)?;
        if candidate.state != command.expected_state {
            return Err(p::Error("candidate review expected state is stale".into()));
        }
        let retraction = (command.decision == p::CandidateReviewDecision::Retract).then(|| {
            let target = p::ObjectRef(candidate.target.0.clone());
            p::CandidateRetraction {
                schema_version: p::SchemaVersion(1),
                lineage: p::LineageRef(format!("workbench-evidence:{}", binding.0)),
                derived_refs: lineage(&events, &target),
                target,
            }
        });
        let review = p::CandidateReviewCommand {
            schema_version: p::SchemaVersion(1),
            run: command.run.clone(),
            candidate: command.candidate.clone(),
            expected_state: command.expected_state,
            decision: command.decision,
            actor: p::Actor::Owner,
            evidence: candidate.evidence,
            retraction,
        };
        review.validate()?;
        self.append_workbench_receipt(&key, &binding, "started", &target)?;
        // The frozen owner review still checks the real candidate and governor.
        // Failure after a partial append is Unknown, never a claim of rollback.
        let result = harness.review_candidate_bound(review);
        let status = if result.is_ok() {
            "confirmed"
        } else {
            let after = self.owner_brain_events_at(None)?;
            let changed = after.iter().any(|event| {
                !events.iter().any(|old| old.event_id == event.event_id)
                    && !matches!(event.payload, p::EventPayload::MemoryMaintenanceApplied(_))
            });
            if changed {
                return previous(&after, &command.command, &key, &binding)?
                    .ok_or_else(|| p::Error("review receipt is missing".into()));
            }
            "rejected"
        };
        self.append_workbench_receipt(&key, &binding, status, &target)?;
        previous(
            &self.owner_brain_events_at(None)?,
            &command.command,
            &key,
            &binding,
        )?
        .ok_or_else(|| p::Error("review receipt is missing".into()))
    }

    fn append_workbench_receipt(
        &self,
        key: &p::SchemaDigest,
        binding: &p::SchemaDigest,
        status: &str,
        target: &p::SchemaDigest,
    ) -> p::Result<p::EventId> {
        self.services().store.append(p::Event::new(
            p::EventId(format!("{RECEIPT_PREFIX}:{}:{status}", key.0)),
            self.services().aggregates.memory_cognition.clone(),
            None,
            p::EventPayload::MemoryMaintenanceApplied(p::MemoryMaintenanceAppliedPayload {
                deltas_ref: p::MemoryDeltasRef(format!(
                    "{RECEIPT_PREFIX}|{}|{}|{status}|{}",
                    key.0, binding.0, target.0
                )),
            }),
            p::SchemaVersion(1),
            now_ms(),
            p::Provenance {
                source: p::Source::OwnerControl,
                actor: p::Actor::Owner,
                trust_tier: p::TrustTier::OwnerInput,
                caused_by: None,
            },
        ))
    }
}

fn previous(
    events: &[p::Event],
    command: &p::IdempotencyKey,
    key: &p::SchemaDigest,
    binding: &p::SchemaDigest,
) -> p::Result<Option<p::WorkbenchCommandReceipt>> {
    let prefix = format!("{RECEIPT_PREFIX}|{}|", key.0);
    let mut receipt = None;
    for event in events {
        let p::EventPayload::MemoryMaintenanceApplied(payload) = &event.payload else {
            continue;
        };
        let Some(value) = payload.deltas_ref.0.strip_prefix(&prefix) else {
            continue;
        };
        if event.provenance.source != p::Source::OwnerControl
            || event.provenance.actor != p::Actor::Owner
            || event.provenance.trust_tier != p::TrustTier::OwnerInput
        {
            return Err(p::Error("workbench receipt provenance is invalid".into()));
        }
        let fields: Vec<_> = value.split('|').collect();
        let [recorded_binding, state, target] = fields.as_slice() else {
            return Err(p::Error("workbench receipt is invalid".into()));
        };
        p::sha256_digest_bytes(&p::SchemaDigest((*target).into()))?;
        if *recorded_binding != binding.0 {
            return Err(p::Error("workbench command identity conflict".into()));
        }
        let status = match *state {
            "started" => p::WorkbenchCommandStatus::Unknown,
            "confirmed" => p::WorkbenchCommandStatus::Confirmed,
            "rejected" => p::WorkbenchCommandStatus::Rejected,
            _ => return Err(p::Error("workbench receipt status is invalid".into())),
        };
        receipt = Some(p::WorkbenchCommandReceipt {
            schema_version: p::SchemaVersion(1),
            command: command.clone(),
            binding: binding.clone(),
            status,
            event: event.event_id.clone(),
        });
    }
    Ok(receipt)
}

pub(super) fn pending_target(events: &[p::Event], target: &p::SchemaDigest) -> bool {
    let mut states = BTreeMap::new();
    for event in events {
        let p::EventPayload::MemoryMaintenanceApplied(payload) = &event.payload else {
            continue;
        };
        let fields: Vec<_> = payload.deltas_ref.0.split('|').collect();
        if let [prefix, key, _binding, state, recorded_target] = fields.as_slice() {
            if *prefix == RECEIPT_PREFIX && *recorded_target == target.0 {
                states.insert(*key, *state);
            }
        }
    }
    states.values().any(|state| *state == "started")
}
