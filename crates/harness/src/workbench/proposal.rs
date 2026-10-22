//! Durable owner control for ordinary proactive proposals.
//!
//! A proposal acknowledgement is not an execution permission.  Adoption is
//! recorded in the brain event stream; any resulting action must still enter
//! the normal goal/plan/policy path.
use super::*;

const RECEIPT_PREFIX: &str = "workbench-proposal-v1";

impl ProductionRuntime {
    pub fn resolve_workbench_proposal(
        &self,
        owner: &p::VerifiedPrincipal,
        command: p::WorkbenchProposalCommand,
    ) -> p::Result<p::WorkbenchCommandReceipt> {
        let _data = self.data_read_guard()?;
        self.require_brain_owner(owner)?;
        command.validate()?;
        let key = p::canonical_digest(&(owner, &command.command))?;
        let binding = p::canonical_digest(&(owner, &command))?;
        let target = p::canonical_digest(&(&command.run, &command.proposal))?;
        let lock = self
            .harness()
            .session_lock(&p::SessionId(format!("workbench-proposal:{}", key.0)))?;
        let _guard = lock
            .lock()
            .map_err(|_| p::Error("workbench proposal control is unavailable".into()))?;
        let target_lock = self.harness().session_lock(&p::SessionId(format!(
            "workbench-proposal-target:{}",
            target.0
        )))?;
        let _target_guard = target_lock
            .lock()
            .map_err(|_| p::Error("workbench proposal control is unavailable".into()))?;

        let events = self.owner_brain_events_at(None)?;
        if let Some(receipt) = previous(&events, &command.command, &key, &binding)? {
            return Ok(receipt);
        }
        if command.defer_until.is_some_and(|at| at <= now_ms()) {
            return Err(p::Error(
                "deferred proposal requires a future trigger".into(),
            ));
        }
        if pending_target(&events, &target) {
            return Err(p::Error(
                "proposal control has an unresolved receipt; inspect the original operation".into(),
            ));
        }
        let emitted = events.iter().find(|event| {
            event.run_id == command.run
                && matches!(
                    &event.payload,
                    p::EventPayload::ProactiveProposalEmitted(payload)
                        if payload.proposal_ref == command.proposal && payload.learning.is_none()
                )
        });
        let Some(_emitted) = emitted else {
            return Err(p::Error(
                "ordinary proactive proposal was not found in the requested run".into(),
            ));
        };
        if events.iter().any(|event| {
            matches!(
                &event.payload,
                p::EventPayload::ProactiveProposalResolved(payload)
                    if payload.proposal_ref == command.proposal
            )
        }) {
            return Err(p::Error(
                "proposal already has a recorded owner outcome".into(),
            ));
        }

        self.append_receipt(&key, &binding, "started", &target)?;
        let feedback = command
            .defer_until
            .map(|at| p::FeedbackRef(format!("workbench-defer-until:{at}")));
        let result = self.harness().resolve_proactive_proposal(
            command.run.clone(),
            command.proposal.clone(),
            command.outcome,
            feedback,
        );
        if result.is_ok() && command.outcome == p::ProposalOutcome::Defer {
            let at = command
                .defer_until
                .ok_or_else(|| p::Error("deferred proposal requires a trigger".into()))?;
            self.services()
                .intentions
                .proactive
                .create(forme_cognition::ProspectiveIntention {
                    schema_version: p::SchemaVersion(1),
                    id: p::IntentionId(format!("workbench-deferred:{}", command.proposal.0)),
                    source: p::IntentionSource::DeferredProposal,
                    trigger: forme_cognition::IntentionTrigger::At(at),
                    state: forme_cognition::IntentionState::Pending,
                    seed: p::SeedRef(command.proposal.0.clone()),
                    provenance: p::Provenance {
                        source: p::Source::Internal,
                        actor: p::Actor::System,
                        trust_tier: p::TrustTier::VerifiedProcess,
                        caused_by: None,
                    },
                    expires_at: None,
                })?;
        }
        let status = if result.is_ok() {
            "confirmed"
        } else {
            let after = self.owner_brain_events_at(None)?;
            let changed = after
                .iter()
                .any(|event| !events.iter().any(|old| old.event_id == event.event_id));
            if changed {
                return previous(&after, &command.command, &key, &binding)?
                    .ok_or_else(|| p::Error("proposal receipt is missing".into()));
            }
            "rejected"
        };
        self.append_receipt(&key, &binding, status, &target)?;
        previous(
            &self.owner_brain_events_at(None)?,
            &command.command,
            &key,
            &binding,
        )?
        .ok_or_else(|| p::Error("proposal receipt is missing".into()))
    }

    fn append_receipt(
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
        let fields: Vec<_> = value.split('|').collect();
        let [recorded_binding, state, target] = fields.as_slice() else {
            return Err(p::Error("proposal receipt is invalid".into()));
        };
        p::sha256_digest_bytes(&p::SchemaDigest((*target).into()))?;
        if *recorded_binding != binding.0 {
            return Err(p::Error("workbench command identity conflict".into()));
        }
        let status = match *state {
            "started" => p::WorkbenchCommandStatus::Unknown,
            "confirmed" => p::WorkbenchCommandStatus::Confirmed,
            "rejected" => p::WorkbenchCommandStatus::Rejected,
            _ => return Err(p::Error("proposal receipt status is invalid".into())),
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

fn pending_target(events: &[p::Event], target: &p::SchemaDigest) -> bool {
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
