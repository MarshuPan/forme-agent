//! Instance-bound ingress authority and the public harness ingress contracts.
use super::{p, AgentHarness, ApprovalTicket};
use std::sync::Arc;

#[derive(Clone, Default)]
pub struct IngressAuthority(Arc<()>);

impl std::fmt::Debug for IngressAuthority {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("IngressAuthority(<instance-bound>)")
    }
}

impl PartialEq for IngressAuthority {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for IngressAuthority {}

impl IngressAuthority {
    pub fn stamp(&self, payload: p::EventPayload, provenance: p::Provenance) -> IngressEvent {
        IngressEvent {
            schema_version: p::SchemaVersion(1),
            payload,
            provenance,
            authority: self.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct IngressEvent {
    schema_version: p::SchemaVersion,
    pub(super) payload: p::EventPayload,
    pub(super) provenance: p::Provenance,
    authority: IngressAuthority,
}

impl IngressEvent {
    pub fn is_authorized_by(&self, authority: &IngressAuthority) -> bool {
        self.schema_version.0 > 0 && self.authority == *authority
    }

    pub fn into_parts(self) -> (p::EventPayload, p::Provenance) {
        (self.payload, self.provenance)
    }

    pub fn payload(&self) -> &p::EventPayload {
        &self.payload
    }

    pub fn provenance(&self) -> &p::Provenance {
        &self.provenance
    }
}

pub trait HarnessIngress: AgentHarness {
    fn ingress_authority(&self) -> IngressAuthority;

    fn submit_ingress(
        &self,
        request: p::RunRequest,
        prelude: Vec<IngressEvent>,
    ) -> p::Result<p::RunId>;
    fn resolve_approval(
        &self,
        ticket: ApprovalTicket,
        grant: forme_approval::ApprovalGrant,
    ) -> p::Result<()>;
    fn append_ingress_events(
        &self,
        run: p::RunId,
        events: Vec<IngressEvent>,
    ) -> p::Result<Vec<p::EventId>>;
    fn result_text(&self, run: p::RunId) -> p::Result<Option<String>>;
}

pub trait HarnessActionIngress: HarnessIngress {
    fn submit_action(
        &self,
        request: p::RunRequest,
        intent: p::ActionIntent,
        envelope: p::AutonomyEnvelope,
        prelude: Vec<IngressEvent>,
    ) -> p::Result<p::RunId>;
}
