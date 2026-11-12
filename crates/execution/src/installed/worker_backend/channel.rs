//! Body resolution remains in Core; only the fixed bytes cross AgentExecution.
use super::*;
pub(super) struct ChannelConsumer {
    installation: String,
    sources: Vec<p::ProductChannelSource>,
    contents: Arc<dyn crate::ContentResolver>,
}
impl WorkerActionBackend {
    pub fn with_channel_delivery(
        mut self,
        installation: String,
        sources: Vec<p::ProductChannelSource>,
        contents: Arc<dyn crate::ContentResolver>,
    ) -> p::Result<Self> {
        p::validate_product_id(&installation)?;
        p::ProductChannelSourceCatalog {
            schema_version: p::SchemaVersion(1),
            sources: sources.clone(),
        }
        .validate()?;
        if self.kind != BackendKind::AppApi
            || self.sandbox_profile_ref != p::MANAGED_CHANNEL_PROFILE
            || sources.is_empty()
            || sources.iter().any(|s| s.delivery.is_none())
        {
            return Err(invalid());
        }
        self.channel = Some(ChannelConsumer {
            installation,
            sources,
            contents,
        });
        Ok(self)
    }
    fn channel_input(
        &self,
        request: p::WorkerDispatchRequest,
        at: p::Timestamp,
    ) -> p::Result<(
        p::ManagedChannelDeliveryInputV1,
        p::ManagedChannelDeliveryGrantV1,
        &p::ProductChannelSource,
    )> {
        let consumer = self.channel.as_ref().ok_or_else(invalid)?;
        let p::ActionParameters::AppApi(spec) = &request
            .execution
            .as_ref()
            .ok_or_else(invalid)?
            .intent
            .parameters
        else {
            return Err(invalid());
        };
        let source = consumer
            .sources
            .iter()
            .find(|s| {
                s.connector_ref == spec.connector.0
                    && Some(&s.participant) == spec.participant.as_ref()
            })
            .ok_or_else(invalid)?;
        let p::AppApiOperation::Mutation {
            body: Some(p::ExternalInput::Content(reference)),
            ..
        } = &spec.operation
        else {
            return Err(invalid());
        };
        let content_bytes = consumer.contents.resolve(reference)?;
        let grant = p::ManagedChannelDeliveryGrantV1::from_request(
            &consumer.installation,
            source,
            &request,
            &content_bytes,
            at,
        )?;
        Ok((
            p::ManagedChannelDeliveryInputV1 {
                schema_version: p::SchemaVersion(1),
                execution: request,
                grant_ref: grant.grant_ref.clone(),
                content_bytes,
            },
            grant,
            source,
        ))
    }
    pub(super) fn check_channel_plan(&self, plan: &ExecutionPlan) -> p::Result<()> {
        if self.sandbox_profile_ref != p::MANAGED_CHANNEL_PROFILE {
            return Ok(());
        }
        let at = now_ms();
        let attempt = build_attempt(
            plan,
            self.owner.clone(),
            self.authority_epoch,
            self.sandbox_profile_ref.clone(),
            at,
        )?;
        self.channel_input(
            p::WorkerDispatchRequest {
                schema_version: p::SchemaVersion(1),
                job: attempt.job,
                permit: attempt.permit,
                execution: Some(execution_spec(plan)),
            },
            at,
        )?;
        Ok(())
    }
    pub(super) fn dispatch_request(
        &self,
        plan: &ExecutionPlan,
        request: p::WorkerDispatchRequest,
    ) -> p::Result<p::WorkerObservation> {
        if self.sandbox_profile_ref != p::MANAGED_CHANNEL_PROFILE {
            return self.transport.dispatch(request);
        }
        if plan.approval_ref.is_none() {
            return Err(invalid());
        }
        let (input, _, _) = self.channel_input(request, now_ms())?;
        self.transport.dispatch_channel(input)
    }
    pub(super) fn verify_channel_result(
        &self,
        plan: &ExecutionPlan,
        attempt: &WorkerAttempt,
        bytes: &[u8],
    ) -> p::Result<()> {
        if self.sandbox_profile_ref != p::MANAGED_CHANNEL_PROFILE {
            return Ok(());
        }
        let (_, grant, source) = self.channel_input(
            p::WorkerDispatchRequest {
                schema_version: p::SchemaVersion(1),
                job: attempt.job.clone(),
                permit: attempt.permit.clone(),
                execution: Some(execution_spec(plan)),
            },
            attempt.permit.issued_at,
        )?;
        super::super::channel_delivery::proxy::verify_receipt(&grant, source, bytes)
    }
}
fn invalid() -> p::Error {
    p::Error("channel body, approval or original receipt cannot be verified".into())
}
