//! Durable intention lifecycle and claim leases, separate from graph storage.
use super::*;

impl<S: EventStore> EventSourcedMemory<S> {
    fn claim_due_matching(
        &self,
        now: p::Timestamp,
        lease_ms: i64,
        schedule_filter: Option<bool>,
        max_claims: usize,
    ) -> Vec<ClaimedIntention> {
        if lease_ms <= 0 || max_claims == 0 {
            return Vec::new();
        }
        let Ok(mut state) = self.lock_state() else {
            return Vec::new();
        };
        let ids = state.intentions.keys().cloned().collect::<Vec<_>>();
        let mut claimed = Vec::new();
        for id in ids {
            if self.is_intention_quarantined(&id) {
                continue;
            }
            if claimed.len() >= max_claims {
                break;
            }
            let Some(snapshot) = state.intentions.get(&id).cloned() else {
                continue;
            };
            if schedule_filter.is_some_and(|scheduled| scheduled != snapshot.schedule.is_some()) {
                continue;
            }
            if matches!(
                snapshot.intention.state,
                IntentionState::Done | IntentionState::Expired | IntentionState::Cancelled
            ) {
                continue;
            }
            if snapshot
                .intention
                .expires_at
                .is_some_and(|expires_at| expires_at <= now)
            {
                if let Ok(resolved_event) = self.append_payload(
                    system_provenance(),
                    intention_resolved_payload(&id, IntentionOutcome::Expired),
                ) {
                    if let Some(record) = state.intentions.get_mut(&id) {
                        record.intention.state = IntentionState::Expired;
                        record.lease_until = None;
                        record.claim_generation = resolved_event;
                    }
                }
                continue;
            }
            let due = match snapshot.intention.state {
                IntentionState::Pending => {
                    matches!(snapshot.intention.trigger, IntentionTrigger::At(at) if at <= now)
                        && snapshot
                            .lease_until
                            .is_none_or(|lease_until| lease_until <= now)
                }
                IntentionState::Fired => snapshot
                    .lease_until
                    .is_none_or(|lease_until| lease_until <= now),
                IntentionState::Done | IntentionState::Expired | IntentionState::Cancelled => false,
            };
            if !due {
                continue;
            }
            let lease_until = now.saturating_add(lease_ms);
            let lease_payload =
                p::EventPayload::MemoryMaintenanceApplied(p::MemoryMaintenanceAppliedPayload {
                    deltas_ref: p::MemoryDeltasRef(format!(
                        "intention-lease|{}|{lease_until}|{}",
                        escape_component(&snapshot.claim_generation.0),
                        escape_component(&id.0),
                    )),
                });
            let Ok((lease_event, lease_claimed)) =
                self.append_payload_once(system_provenance(), lease_payload)
            else {
                continue;
            };
            if !lease_claimed {
                continue;
            }
            if let Some(record) = state.intentions.get_mut(&id) {
                record.lease_until = Some(lease_until);
                record.claim_generation = lease_event;
            }
            let Ok(claim_event) = self.append_payload(
                system_provenance(),
                intention_resolved_payload(&id, IntentionOutcome::Fired),
            ) else {
                continue;
            };
            if let Some(record) = state.intentions.get_mut(&id) {
                record.intention.state = IntentionState::Fired;
                record.claim_generation = claim_event.clone();
                claimed.push(ClaimedIntention {
                    schema_version: p::SchemaVersion(1),
                    intention: record.intention.clone(),
                    schedule: record.schedule.clone(),
                    lease_until,
                    claim_event,
                });
            }
        }
        claimed
    }
}

impl<S> IntentionStore for EventSourcedMemory<S>
where
    S: EventStore,
{
    fn create(&self, intention: ProspectiveIntention) -> p::Result<p::IntentionId> {
        validate_intention(&intention)?;
        let mut state = self.lock_state()?;
        if let Some(existing) = state.intentions.get(&intention.id) {
            return if existing.intention == intention {
                Ok(intention.id)
            } else {
                Err(p::Error("intention id collision".into()))
            };
        }
        let created_event = self.append_payload(
            intention.provenance.clone(),
            p::EventPayload::ProspectiveIntentionCreated(p::ProspectiveIntentionCreatedPayload {
                intention_id: intention.id.clone(),
                source: intention.source,
                trigger: intention_storage_ref(&intention),
                schedule: None,
                goal_frame: None,
            }),
        )?;
        state.intentions.insert(
            intention.id.clone(),
            IntentionRecord {
                intention: intention.clone(),
                schedule: None,
                lease_until: None,
                claim_generation: created_event,
            },
        );
        Ok(intention.id)
    }

    fn create_schedule(&self, command: p::ScheduleCommand) -> p::Result<p::IntentionId> {
        command.validate()?;
        let intention = intention_from_protocol(&command.intention);
        validate_intention(&intention)?;
        let binding = command.binding();
        let mut state = self.lock_state()?;
        if let Some(existing) = state.intentions.get(&intention.id) {
            return if same_intention_definition(&existing.intention, &intention)
                && existing.schedule.as_ref() == Some(&binding)
            {
                Ok(intention.id)
            } else {
                Err(p::Error("intention id collision".into()))
            };
        }
        let created_event = self.append_payload(
            intention.provenance.clone(),
            p::EventPayload::ProspectiveIntentionCreated(p::ProspectiveIntentionCreatedPayload {
                intention_id: intention.id.clone(),
                source: intention.source,
                trigger: intention_storage_ref(&intention),
                schedule: Some(binding.clone()),
                goal_frame: None,
            }),
        )?;
        state.intentions.insert(
            intention.id.clone(),
            IntentionRecord {
                intention: intention.clone(),
                schedule: Some(binding),
                lease_until: None,
                claim_generation: created_event,
            },
        );
        Ok(intention.id)
    }

    fn claim_due(&self, now: p::Timestamp, lease_ms: i64) -> Vec<ClaimedIntention> {
        self.claim_due_matching(now, lease_ms, None, usize::MAX)
    }

    fn claim_due_unscheduled(&self, now: p::Timestamp, lease_ms: i64) -> Vec<ClaimedIntention> {
        self.claim_due_matching(now, lease_ms, Some(false), usize::MAX)
    }

    fn claim_due_scheduled(
        &self,
        now: p::Timestamp,
        lease_ms: i64,
        max_claims: usize,
    ) -> Vec<ClaimedIntention> {
        self.claim_due_matching(now, lease_ms, Some(true), max_claims)
    }

    fn resolve(&self, id: p::IntentionId, outcome: IntentionOutcome) -> p::Result<()> {
        if self.is_intention_quarantined(&id) && outcome != IntentionOutcome::Cancelled {
            return Err(p::Error(
                "restored intention requires a new owner plan".into(),
            ));
        }
        let mut state = self.lock_state()?;
        let current = state
            .intentions
            .get(&id)
            .ok_or_else(|| p::Error("intention is not registered".into()))?
            .intention
            .state;
        let target = intention_state(outcome);
        if current == target {
            return Ok(());
        }
        if !valid_intention_transition(current, target) {
            return Err(p::Error("intention transition is not allowed".into()));
        }
        let resolved_event = self.append_payload(
            system_provenance(),
            intention_resolved_payload(&id, outcome),
        )?;
        if let Some(record) = state.intentions.get_mut(&id) {
            record.intention.state = target;
            if target != IntentionState::Fired {
                record.lease_until = None;
            }
            record.claim_generation = resolved_event;
        }
        Ok(())
    }

    fn list_scheduled(&self) -> Vec<ScheduledIntentionRecord> {
        self.lock_state()
            .map(|state| {
                state
                    .intentions
                    .values()
                    .filter_map(|record| {
                        let binding = record.schedule.clone()?;
                        Some(ScheduledIntentionRecord {
                            schema_version: p::SchemaVersion(1),
                            command: schedule_command(&record.intention, &binding),
                            lease_until: record.lease_until,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
}
