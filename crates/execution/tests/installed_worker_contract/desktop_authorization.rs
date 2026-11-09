use super::*;
use forme_execution::installed::{
    DesktopAuthority, DesktopSessionCatalogProvider, ObservedDesktopSession,
};

struct Directory(ObservedDesktopSession);
impl DesktopSessionCatalogProvider for Directory {
    fn sessions(&self) -> p::Result<Vec<ObservedDesktopSession>> {
        Ok(vec![self.0.clone()])
    }
    fn recheck(&self, original: &ObservedDesktopSession) -> p::Result<()> {
        if original == &self.0 {
            Ok(())
        } else {
            Err(p::Error("fixture session changed".into()))
        }
    }
}
pub(super) fn authorize(
    temp: &Temp,
    host: WorkerExecutorHost<FakeIsolation, FakeDesktopHelper>,
    agent: &forme_execution::installed::HostAuth,
    job: &p::WorkerJob,
    permit: &p::WorkerPermit,
    mut execution: p::WorkerExecutionSpec,
) -> (
    WorkerExecutorHost<FakeIsolation, FakeDesktopHelper>,
    p::WorkerExecutionSpec,
) {
    let original = execution.desktop_session.take().unwrap();
    let directory = Arc::new(Directory(ObservedDesktopSession {
        schema_version: p::SchemaVersion(1),
        helper_ref: original.helper_ref,
        platform: original.platform,
        label: "Fixture session".into(),
        os_user_ref: original.os_user_ref,
        os_session_ref: original.os_session_ref,
        session_generation: original.session_generation,
        desktop_ref: original.desktop_ref,
        capabilities: original.capabilities,
    }));
    let authority = Arc::new(
        DesktopAuthority::open(
            &temp.worker_journal().with_extension("desktop.jsonl"),
            "installation:fixture".into(),
            job.owner_ref.clone(),
            job.authority_epoch,
            directory,
        )
        .unwrap(),
    );
    let host = host.with_desktop_authority(authority.clone());
    let pending = p::DesktopPendingAction {
        schema_version: p::SchemaVersion(1),
        pending_ref: "pending:one".into(),
        owner_ref: job.owner_ref.clone(),
        authority_epoch: job.authority_epoch,
        run_ref: p::RunId("run:one".into()),
        approval_ref: p::ApprovalId("approval:one".into()),
        action_ref: job.action_ref.clone(),
        plan_digest: job.plan_digest.clone(),
        normalized_action_digest: execution.normalized_action_digest.clone(),
        affected_resources: job.affected_resources.clone(),
        capability: p::DesktopCapability::PointerClick,
        policy_version: p::Version(1),
        tool_schema_version: p::Version(1),
        expires_at: 900,
    };
    host.register_desktop_pending(agent, pending, 3).unwrap();
    let catalog = authority.sessions(4).unwrap();
    let consent = authority
        .consent(
            p::DesktopConsentRequest {
                schema_version: p::SchemaVersion(1),
                operation_id: "consent:one".into(),
                expected_revision: catalog.catalog_revision,
                pending_ref: "pending:one".into(),
                catalog_revision: catalog.catalog_revision,
                catalog_digest: catalog.catalog_digest,
                session_ref: catalog.entries[0].session_ref.clone(),
                expires_at: 900,
            },
            5,
        )
        .unwrap();
    let grant = host
        .acquire_desktop_grant(
            agent,
            p::DesktopGrantRequest {
                schema_version: p::SchemaVersion(1),
                operation_id: "issue:one".into(),
                consent_ref: consent.consent_ref,
                job: job.clone(),
                permit: permit.clone(),
                execution: execution.clone(),
            },
            6,
        )
        .unwrap();
    execution.desktop_session = Some(grant.binding);
    (host, execution)
}
