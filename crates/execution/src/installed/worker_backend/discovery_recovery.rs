//! Read the first discovery job without reserving, dispatching or cancelling.
use super::*;

pub(super) type Recorder = Arc<dyn Fn(&p::WorkerDispatchRequest) -> p::Result<()> + Send + Sync>;

impl WorkerActionBackend {
    /// Managed composition retains the exact request before ActionStarted.
    /// A retention failure prevents the first transport dispatch.
    pub fn with_discovery_dispatch_recorder<F>(mut self, recorder: F) -> Self
    where
        F: Fn(&p::WorkerDispatchRequest) -> p::Result<()> + Send + Sync + 'static,
    {
        self.discovery_dispatch = Some(Arc::new(recorder));
        self
    }

    pub(super) fn record_discovery_dispatch(
        &self,
        request: &p::WorkerDispatchRequest,
    ) -> p::Result<()> {
        if request
            .execution
            .as_ref()
            .is_some_and(|e| matches!(e.intent.parameters, p::ActionParameters::McpDiscovery(_)))
        {
            if let Some(recorder) = &self.discovery_dispatch {
                recorder(request)?;
            }
        }
        Ok(())
    }

    /// Returns buffered observations only after a complete protected original
    /// exists. The caller must validate its authority and append them with CAS.
    /// Expiry never renews the permit: validation uses its original issue time.
    pub fn read_discovery_original(
        &self,
        plan: &ExecutionPlan,
        request: &p::WorkerDispatchRequest,
    ) -> p::Result<Vec<p::EventPayload>> {
        validate_plan_for(plan, BackendKind::Mcp)?;
        request.validate_at(request.permit.issued_at)?;
        if self.kind != BackendKind::Mcp
            || !matches!(plan.intent.parameters, p::ActionParameters::McpDiscovery(_))
            || plan.budget != self.budget
            || plan.timeout != self.timeout
            || request.job.owner_ref != self.owner
            || request.job.authority_epoch != self.authority_epoch
            || request.job.sandbox_profile_ref != self.sandbox_profile_ref
            || self.sandbox_profile_ref != p::MANAGED_MCP_PROGRAM_PROFILE
            || request.execution.as_ref() != Some(&execution_spec(plan))
        {
            return Err(invalid());
        }
        self.require_output_consumer(plan)?;
        let observation = self.transport.probe(&request.job.job_id)?;
        validate_observation(&observation, &request.job)?;
        if observation.state != p::WorkerState::Completed
            || observation.outcome != Some(p::WorkerOutcome::Completed)
        {
            return Err(invalid());
        }
        let attempt = WorkerAttempt {
            job: request.job.clone(),
            permit: request.permit.clone(),
            dispatch_attempted: true,
            desktop: None,
        };
        let sink = EventSink::default();
        let result = self.result_for(plan, &attempt, &observation, &sink)?;
        let events = sink.take();
        if result.status != crate::ActionStatus::Completed
            || events.iter().any(|event| {
                !matches!(
                    event,
                    p::EventPayload::ActionOutputDelta(_) | p::EventPayload::ActionCompleted(_)
                )
            })
            || events
                .iter()
                .filter(|e| e.kind() == p::EventKind::ActionCompleted)
                .count()
                != 1
        {
            return Err(invalid());
        }
        Ok(events)
    }
}

fn invalid() -> p::Error {
    p::Error("original discovery result is not available for read-only recovery".into())
}
