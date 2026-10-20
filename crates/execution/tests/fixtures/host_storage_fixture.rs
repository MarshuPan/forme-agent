//! Repository-owned crash fixture, not a product daemon or installed service.
#![forbid(unsafe_code)]

use std::io::{self, Write};
use std::path::PathBuf;

use forme_execution::installed::{
    HostManagement, NativeServiceManager, ProductHostBroker, ProductJournal,
};
use forme_protocol as p;

fn main() {
    let path = PathBuf::from(std::env::args_os().nth(1).expect("fixture journal path"));
    if std::env::args().nth(2).as_deref() == Some("--broker") {
        broker_crash_fixture(path);
        return;
    }
    let effect = path.with_extension("effect-count");
    let journal = ProductJournal::open(&path, "installation-1").expect("fixture writer");
    journal
        .plan(
            p::MaintenanceCommand {
                schema_version: p::SchemaVersion(1),
                operation_id: "operation-1".into(),
                idempotency_key: "command-1".into(),
                expected_revision: 1,
                actor_proof: p::MaintenanceProofRef {
                    schema_version: p::SchemaVersion(1),
                    proof_ref: "proof-fixture".into(),
                },
                kind: p::MaintenanceKind::StartCore,
                requested_deadline: 1000,
            },
            p::MaintenanceActorClass::LocalAdministrator,
            p::MaintenanceReferences {
                schema_version: p::SchemaVersion(1),
                original_release_ref: "0.1.0".into(),
                target_release_ref: "0.1.0".into(),
                data_candidate_ref: None,
                backup_ref: None,
            },
            1,
        )
        .expect("fixture plan");
    journal
        .begin_stage(
            "operation-1",
            2,
            p::MaintenanceStage::Validated,
            p::MaintenanceActorClass::LocalAdministrator,
            2,
        )
        .expect("fixture intent");
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(effect)
        .expect("fixture once-only effect");
    file.write_all(b"1").expect("fixture effect count");
    file.sync_all().expect("fixture effect sync");
    println!("host-intent-durable");
    io::stdout().flush().expect("fixture marker");
    let mut line = String::new();
    let _ = io::stdin().read_line(&mut line);
}

fn broker_crash_fixture(path: PathBuf) {
    struct CrashServices(PathBuf);
    impl NativeServiceManager for CrashServices {
        fn inspect(&self, service: p::BuiltinServiceId) -> p::Result<p::NativeServiceStatus> {
            Ok(p::NativeServiceStatus {
                schema_version: p::SchemaVersion(1),
                service,
                state: p::NativeServiceState::Stopped,
            })
        }
        fn apply(&self, _change: p::ValidatedServiceChange) -> p::Result<p::NativeReceipt> {
            let mut effect = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(self.0.with_extension("effect-count"))
                .expect("once-only broker effect");
            effect.write_all(b"1").unwrap();
            effect.sync_all().unwrap();
            println!("host-intent-durable");
            io::stdout().flush().unwrap();
            let mut line = String::new();
            let _ = io::stdin().read_line(&mut line);
            Err(p::Error("fixture outcome remains unverified".into()))
        }
    }
    const TOKEN: &str = "host-token-abcdefghijklmnopqrstuvwxyz-0123456789";
    let broker = ProductHostBroker::open_with_service_manager(
        &path,
        "installation-1",
        TOKEN.into(),
        p::ProductTarget::WindowsX86_64,
        "0.1.0".into(),
        CrashServices(path.clone()),
    )
    .unwrap();
    let auth = broker.authenticate(TOKEN).unwrap();
    broker
        .plan(
            &auth,
            p::HostManagementRequest {
                schema_version: p::SchemaVersion(1),
                command: p::MaintenanceCommand {
                    schema_version: p::SchemaVersion(1),
                    operation_id: "operation-1".into(),
                    idempotency_key: "command-1".into(),
                    expected_revision: 1,
                    actor_proof: p::MaintenanceProofRef {
                        schema_version: p::SchemaVersion(1),
                        proof_ref: p::canonical_digest(&TOKEN).unwrap().0,
                    },
                    kind: p::MaintenanceKind::StartCore,
                    requested_deadline: 1_000_000,
                },
                references: p::MaintenanceReferences {
                    schema_version: p::SchemaVersion(1),
                    original_release_ref: "0.1.0".into(),
                    target_release_ref: "0.1.0".into(),
                    backup_ref: None,
                    data_candidate_ref: None,
                },
            },
            1,
        )
        .unwrap();
    let _ = broker.execute(&auth, "operation-1", 2);
}
