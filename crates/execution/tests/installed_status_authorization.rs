//! Read-only status shares a safe projection, never privileged operation authority.
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use forme_execution::installed::{
    HostCredentials, HostManagement, NativeServiceManager, ProductHostBroker,
};
use forme_protocol as p;

const ADMIN: &str = "status-fixture-local-administrator-0123456789";
const WEB: &str = "status-fixture-web-configuration-0123456789";
const AGENT: &str = "status-fixture-agent-execution-0123456789";

#[derive(Clone, Default)]
struct Services {
    inspections: Arc<AtomicUsize>,
    changes: Arc<AtomicUsize>,
}

impl NativeServiceManager for Services {
    fn inspect(&self, service: p::BuiltinServiceId) -> p::Result<p::NativeServiceStatus> {
        self.inspections.fetch_add(1, Ordering::SeqCst);
        Ok(p::NativeServiceStatus {
            schema_version: p::SchemaVersion(1),
            service,
            state: p::NativeServiceState::Stopped,
        })
    }

    fn apply(&self, _change: p::ValidatedServiceChange) -> p::Result<p::NativeReceipt> {
        self.changes.fetch_add(1, Ordering::SeqCst);
        Err(p::Error("status fixture forbids service changes".into()))
    }
}

fn broker(root: &Path, web: &str, services: Services) -> ProductHostBroker<Services> {
    let credentials = HostCredentials::new(ADMIN.into(), Some(AGENT.into()))
        .unwrap()
        .with_web_configuration(Some(web.into()))
        .unwrap();
    ProductHostBroker::open_with_service_manager_and_config(
        &root.join("product.jsonl"),
        "install:status-fixture",
        credentials,
        p::ProductTarget::WindowsX86_64,
        "status-fixture".into(),
        None,
        services,
    )
    .unwrap()
}

#[test]
fn configuration_identity_reads_status_without_gaining_privileged_operations() {
    let directory = tempfile::tempdir().unwrap();
    let services = Services::default();
    let host = broker(directory.path(), WEB, services.clone());
    let journal = directory.path().join("product.jsonl");
    drop(host);
    let before = fs::read(&journal).unwrap();
    let host = broker(directory.path(), WEB, services.clone());
    let web = host.authenticate_web_configuration(WEB).unwrap();
    let admin = host.authenticate(ADMIN).unwrap();

    let expected = host.status(&admin).unwrap();
    for _ in 0..3 {
        let observed = host.status(&web).unwrap();
        assert_eq!(observed, expected);
        assert_eq!(observed.core_health, p::CoreHealth::Stopped);
        assert_eq!(observed.active_config_revision, 0);
        assert_eq!(observed.acknowledged_config_revision, None);
    }

    let request = p::HostManagementRequest {
        schema_version: p::SchemaVersion(1),
        command: p::MaintenanceCommand {
            schema_version: p::SchemaVersion(1),
            operation_id: "status:forbidden-start".into(),
            idempotency_key: "status:forbidden-start".into(),
            expected_revision: 1,
            actor_proof: p::MaintenanceProofRef {
                schema_version: p::SchemaVersion(1),
                proof_ref: p::canonical_digest(&WEB).unwrap().0,
            },
            kind: p::MaintenanceKind::StartCore,
            requested_deadline: 1_000_000,
        },
        references: p::MaintenanceReferences {
            schema_version: p::SchemaVersion(1),
            original_release_ref: "status-fixture".into(),
            target_release_ref: "status-fixture".into(),
            data_candidate_ref: None,
            backup_ref: None,
        },
    };
    request.validate().unwrap();
    let denied = "host management authorization is invalid";
    assert_eq!(host.plan(&web, request, 100).unwrap_err().0, denied);
    assert_eq!(
        host.execute(&web, "status:forbidden-start", 100)
            .unwrap_err()
            .0,
        denied
    );
    assert_eq!(
        host.operation(&web, "status:forbidden-start")
            .unwrap_err()
            .0,
        denied
    );
    drop(host);
    assert_eq!(fs::read(&journal).unwrap(), before);
    assert_eq!(services.changes.load(Ordering::SeqCst), 0);
}

#[test]
fn status_rejects_agent_and_foreign_configuration_proofs_before_observation() {
    let directory = tempfile::tempdir().unwrap();
    let services = Services::default();
    let host = broker(directory.path(), WEB, services.clone());
    let foreign_directory = tempfile::tempdir().unwrap();
    let foreign_web = format!("{WEB}-foreign");
    let foreign = broker(foreign_directory.path(), &foreign_web, Services::default());
    let journal = directory.path().join("product.jsonl");
    drop(host);
    let before = fs::read(&journal).unwrap();
    let host = broker(directory.path(), WEB, services.clone());

    for identity in [
        host.authenticate_agent(AGENT).unwrap(),
        foreign
            .authenticate_web_configuration(&foreign_web)
            .unwrap(),
    ] {
        assert!(host.status(&identity).is_err());
    }

    drop(host);
    assert_eq!(fs::read(&journal).unwrap(), before);
    assert_eq!(services.inspections.load(Ordering::SeqCst), 0);
    assert_eq!(services.changes.load(Ordering::SeqCst), 0);
}
