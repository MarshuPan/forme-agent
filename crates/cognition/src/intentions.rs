//! Intention service adapters from the memory owner into the cognition API.
#![forbid(unsafe_code)]

use std::sync::Arc;

use forme_memory as memory;
use forme_protocol as p;
use forme_store::EventStore;

pub trait ProspectiveIntentions: Send + Sync {
    fn create(&self, intention: memory::ProspectiveIntention) -> p::Result<p::IntentionId>;
    fn claim_due(&self, now: p::Timestamp, lease_ms: i64) -> Vec<memory::ClaimedIntention>;
    fn resolve(&self, id: p::IntentionId, outcome: memory::IntentionOutcome) -> p::Result<()>;
}

impl<S> ProspectiveIntentions for memory::EventSourcedMemory<S>
where
    S: EventStore + Send + Sync + 'static,
{
    fn create(&self, intention: memory::ProspectiveIntention) -> p::Result<p::IntentionId> {
        memory::IntentionStore::create(self, intention)
    }

    fn claim_due(&self, now: p::Timestamp, lease_ms: i64) -> Vec<memory::ClaimedIntention> {
        memory::IntentionStore::claim_due_unscheduled(self, now, lease_ms)
    }

    fn resolve(&self, id: p::IntentionId, outcome: memory::IntentionOutcome) -> p::Result<()> {
        memory::IntentionStore::resolve(self, id, outcome)
    }
}

pub trait ScheduledIntentions: Send + Sync {
    fn schedule(&self, command: p::ScheduleCommand) -> p::Result<p::IntentionId>;
    fn claim_due(
        &self,
        now: p::Timestamp,
        lease_ms: i64,
        max_claims: usize,
    ) -> p::Result<Vec<p::ScheduleClaim>>;
    fn resolve(&self, id: p::IntentionId, outcome: p::IntentionOutcome) -> p::Result<()>;
    fn list(&self) -> p::Result<Vec<p::ScheduledJob>>;
}

impl<S> ScheduledIntentions for memory::EventSourcedMemory<S>
where
    S: EventStore + Send + Sync + 'static,
{
    fn schedule(&self, command: p::ScheduleCommand) -> p::Result<p::IntentionId> {
        memory::IntentionStore::create_schedule(self, command)
    }

    fn claim_due(
        &self,
        now: p::Timestamp,
        lease_ms: i64,
        max_claims: usize,
    ) -> p::Result<Vec<p::ScheduleClaim>> {
        Ok(
            memory::IntentionStore::claim_due_scheduled(self, now, lease_ms, max_claims)
                .into_iter()
                .filter_map(|claim| claim.scheduled_claim())
                .collect(),
        )
    }

    fn resolve(&self, id: p::IntentionId, outcome: p::IntentionOutcome) -> p::Result<()> {
        memory::IntentionStore::resolve(
            self,
            id,
            match outcome {
                p::IntentionOutcome::Fired => memory::IntentionOutcome::Fired,
                p::IntentionOutcome::Done => memory::IntentionOutcome::Done,
                p::IntentionOutcome::Expired => memory::IntentionOutcome::Expired,
                p::IntentionOutcome::Cancelled => memory::IntentionOutcome::Cancelled,
            },
        )
    }

    fn list(&self) -> p::Result<Vec<p::ScheduledJob>> {
        Ok(memory::IntentionStore::list_scheduled(self)
            .into_iter()
            .map(|record| {
                let binding = record.command.binding();
                let manual_review = self.is_intention_quarantined(&record.command.intention.id);
                p::ScheduledJob {
                    schema_version: record.schema_version,
                    intention: record.command.intention,
                    binding,
                    lease_until: record.lease_until,
                    run: None,
                    run_status: None,
                    manual_review,
                }
            })
            .collect())
    }
}

pub struct IntentionServices {
    pub proactive: Arc<dyn ProspectiveIntentions>,
    pub scheduled: Arc<dyn ScheduledIntentions>,
}

pub fn event_sourced_intention_services<S>(
    store: Arc<S>,
    aggregate_run: p::RunId,
) -> p::Result<IntentionServices>
where
    S: EventStore + Send + Sync + 'static,
{
    event_sourced_intention_services_with_quarantine(store, aggregate_run, Default::default())
}

pub fn event_sourced_intention_services_with_quarantine<S>(
    store: Arc<S>,
    aggregate_run: p::RunId,
    quarantine: std::collections::BTreeSet<p::IntentionId>,
) -> p::Result<IntentionServices>
where
    S: EventStore + Send + Sync + 'static,
{
    let intentions = Arc::new(
        memory::EventSourcedMemory::open(store, aggregate_run)?
            .with_quarantined_intentions(quarantine),
    );
    let proactive: Arc<dyn ProspectiveIntentions> = intentions.clone();
    let scheduled: Arc<dyn ScheduledIntentions> = intentions;
    Ok(IntentionServices {
        proactive,
        scheduled,
    })
}
