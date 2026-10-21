//! Gateway runtime boundary; public facade remains in the crate root.
use super::*;

impl LocalGateway<ReactiveHarness> {
    pub fn from_production_runtime(runtime: Arc<ProductionRuntime>) -> p::Result<Self> {
        let owner = p::VerifiedPrincipal(runtime.owner().0.clone());
        let agent_profile = runtime.manifest().agent_profile.clone();
        let harness = runtime.harness();
        let mut gateway = Self::new(harness, owner, agent_profile)?;
        gateway.production_runtime = Some(runtime);
        Ok(gateway)
    }
}

impl<H> Gateway for LocalGateway<H>
where
    H: HarnessIngress + Send + Sync,
{
    fn submit(&self, event: CommunicationEvent) -> p::Result<p::RunId> {
        let auth = match &event.participant {
            ParticipantProfile::Owner => {
                AuthContext::local_owner(self.owner.clone(), now_ms().saturating_add(60_000))
            }
            ParticipantProfile::External(participant)
            | ParticipantProfile::ExternalAgent(participant) => AuthContext {
                schema_version: p::SchemaVersion(1),
                principal: AuthPrincipal::External(participant.clone()),
                method: forme_communication::AuthMethod::MutualChannel,
                expires_at: now_ms().saturating_add(60_000),
            },
            ParticipantProfile::AgentRepresentingOwner => AuthContext {
                schema_version: p::SchemaVersion(1),
                principal: AuthPrincipal::AgentRepresentingOwner(self.owner.clone()),
                method: forme_communication::AuthMethod::LocalSession,
                expires_at: now_ms().saturating_add(60_000),
            },
        };
        self.submit_event_with_auth(event, auth)
    }

    fn stream_events(&self, run: p::RunId) -> EventStream {
        self.harness.stream_events(run)
    }

    fn approve(&self, ticket: ApprovalTicket, grant: ApprovalGrant) -> p::Result<()> {
        if grant.approver != self.owner {
            return Err(p::Error(
                "approval principal does not match the local owner".into(),
            ));
        }
        self.harness.resolve_approval(ticket, grant)
    }
}

impl<H> RunGateway for LocalGateway<H>
where
    H: HarnessIngress + Send + Sync,
{
    fn submit_run(&self, request: p::RunRequest) -> p::Result<p::RunId> {
        LocalGateway::submit_run(
            self,
            request,
            AuthContext::local_owner(self.owner.clone(), now_ms().saturating_add(60_000)),
        )
    }

    fn stream_run(&self, run: p::RunId) -> EventStream {
        Gateway::stream_events(self, run)
    }

    fn wait_run(&self, run: p::RunId) -> p::Result<p::RunResult> {
        self.wait(run)
    }

    fn answer(&self, run: p::RunId) -> p::Result<Option<String>> {
        LocalGateway::answer(self, run)
    }

    fn agent_profile(&self) -> p::AgentProfileRef {
        self.agent_profile.clone()
    }

    fn runtime_manifest_digest(&self) -> p::Result<p::SchemaDigest> {
        self.production_runtime
            .as_ref()
            .map(|runtime| runtime.manifest().digest.clone())
            .ok_or_else(|| p::Error("this gateway is not backed by a production runtime".into()))
    }
}
