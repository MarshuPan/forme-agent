//! State-machine fixtures; native service/worker results below are simulated.
use super::*;
use crate::installed::{
    product_config::tests::{fixture, patch},
    RestoreWorkerProvider, RestoreWorkerSession,
};
use std::{
    fs,
    path::PathBuf,
    sync::Weak,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

const WEB: &str = "restore-web-fixture-abcdefghijklmnopqrstuvwxyz";
const ADMIN: &str = "restore-admin-fixture-abcdefghijklmnopqrstuvwxyz";
const OP: &str = "restore:first";
type Host = ProductHostBroker<Services>;

fn digest(value: &str) -> p::SchemaDigest {
    p::canonical_digest(&value).unwrap()
}
fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

struct Effects {
    host: Mutex<Weak<Host>>,
    calls: Mutex<Vec<String>>,
    state: Mutex<p::NativeServiceState>,
    fault: &'static str,
    ack: PathBuf,
    admission: PathBuf,
    binding: p::ProductDataBinding,
}
impl Effects {
    fn call(&self, name: &str, stage: p::MaintenanceStage) -> p::Result<()> {
        let host = self.host.lock().unwrap().upgrade().unwrap();
        assert_eq!(
            host.journal.operation(OP)?.pending_stage,
            Some(stage),
            "effect before stage intent"
        );
        self.calls.lock().unwrap().push(name.into());
        if self.fault == name {
            return Err(p::Error("fixture lost result after possible effect".into()));
        }
        Ok(())
    }
}
#[derive(Clone)]
struct Services(Arc<Effects>);
impl NativeServiceManager for Services {
    fn inspect(&self, service: p::BuiltinServiceId) -> p::Result<p::NativeServiceStatus> {
        Ok(p::NativeServiceStatus {
            schema_version: p::SchemaVersion(1),
            service,
            state: *self.0.state.lock().unwrap(),
        })
    }
    fn apply(&self, change: p::ValidatedServiceChange) -> p::Result<p::NativeReceipt> {
        let state = match change.action {
            p::NativeServiceAction::Stop => {
                self.0.call("stop", p::MaintenanceStage::Quiescing)?;
                if self.0.fault == "unknown_stop" {
                    p::NativeServiceState::Unknown
                } else {
                    p::NativeServiceState::Stopped
                }
            }
            p::NativeServiceAction::Start => {
                self.0.call("start", p::MaintenanceStage::Checking)?;
                let held: p::ProductWorkAdmission =
                    serde_json::from_slice(&fs::read(&self.0.admission).unwrap()).unwrap();
                assert!(!held.released);
                let mut identity = p::ProductDataAcknowledgement {
                    schema_version: p::SchemaVersion(1),
                    owner_digest: self.0.binding.owner_digest.clone(),
                    store_identity: self.0.binding.store_identity.clone(),
                    execution_fence_digest: self.0.binding.execution_fence_digest.clone(),
                    composition_digest: digest("actual-fixture-composition"),
                };
                if self.0.fault == "wrong_ack" {
                    identity.store_identity = digest("foreign-store");
                }
                let ack = p::ProductConfigAcknowledgement {
                    schema_version: p::SchemaVersion(1),
                    install_ref: "install:config-test".into(),
                    revision: 2,
                    data_identity: Some(identity),
                };
                fs::write(&self.0.ack, serde_json::to_vec(&ack).unwrap()).unwrap();
                p::NativeServiceState::Running
            }
        };
        *self.0.state.lock().unwrap() = state;
        Ok(p::NativeReceipt {
            schema_version: p::SchemaVersion(1),
            service: change.service,
            requested: change.action,
            observed: state,
        })
    }
}
struct Worker {
    install: p::InstallationManifest,
    effects: Arc<Effects>,
}
struct Session {
    request: p::ProductRestoreRequest,
    effects: Arc<Effects>,
}
impl RestoreWorkerProvider for Worker {
    fn installation(&self) -> &p::InstallationManifest {
        &self.install
    }
    fn preflight(&self) -> p::Result<()> {
        Ok(())
    }
    fn start(
        &self,
        request: p::ProductRestoreRequest,
        _deadline: Instant,
    ) -> p::Result<Box<dyn RestoreWorkerSession>> {
        self.effects
            .call("validate", p::MaintenanceStage::BackedUp)?;
        Ok(Box::new(Session {
            request,
            effects: self.effects.clone(),
        }))
    }
}
impl Session {
    fn response(&self, result: p::ProductRestoreWorkerResult) -> p::ProductRestoreWorkerResponse {
        p::ProductRestoreWorkerResponse {
            schema_version: p::SchemaVersion(1),
            operation_id: self.request.operation_id.clone(),
            command_digest: self.request.command_digest.clone(),
            result,
        }
    }
}
impl RestoreWorkerSession for Session {
    fn validated(&self) -> p::Result<p::ProductRestoreWorkerResponse> {
        let mut binding = self.effects.binding.clone();
        if self.effects.fault == "foreign_candidate" {
            binding.owner_digest = digest("foreign-owner");
        }
        Ok(self.response(p::ProductRestoreWorkerResult::Validated {
            binding,
            current_store_identity: self.effects.binding.store_identity.clone(),
            current_database_digest: digest("current-db"),
        }))
    }
    fn step(&mut self, step: p::ProductRestoreStep) -> p::Result<p::ProductRestoreWorkerResponse> {
        let result = match step {
            p::ProductRestoreStep::Backup => {
                self.effects.call("backup", p::MaintenanceStage::BackedUp)?;
                p::ProductRestoreWorkerResult::BackedUp {
                    backup_ref: "pre-restore.db".into(),
                    manifest_digest: digest("backup"),
                }
            }
            p::ProductRestoreStep::FenceSessions => {
                self.effects.call("fence", p::MaintenanceStage::Switching)?;
                p::ProductRestoreWorkerResult::SessionsFenced {
                    revision: p::AuthRevision {
                        schema_version: p::SchemaVersion(1),
                        credential_revision: 1,
                        auth_epoch: 2,
                    },
                }
            }
            p::ProductRestoreStep::Release => {
                self.effects
                    .call("release", p::MaintenanceStage::Switching)?;
                p::ProductRestoreWorkerResult::Released {}
            }
        };
        Ok(self.response(result))
    }
}
fn installation(root: &Path) -> p::InstallationManifest {
    let target = p::ProductTarget::WindowsX86_64;
    p::InstallationManifest {
        schema_version: p::SchemaVersion(1),
        install_id: "install:config-test".into(),
        target,
        release_id: "0.1.0".into(),
        release_digest: digest("release"),
        created_at: now(),
        paths: p::ProductPaths {
            schema_version: p::SchemaVersion(1),
            releases: root.join("releases"),
            config: root.join("config"),
            brain: root.join("brain"),
            web_auth: root.join("web-auth"),
            host_state: root.join("host"),
            keys: root.join("keys"),
            logs: root.join("logs"),
            backups: root.join("backups"),
            workspaces: root.join("workspaces"),
            ipc: root.join("ipc"),
        },
        services: [
            p::BuiltinServiceId::Host,
            p::BuiltinServiceId::Web,
            p::BuiltinServiceId::Brain,
        ]
        .into_iter()
        .map(|service| p::ServiceIdentityBinding {
            schema_version: p::SchemaVersion(1),
            service,
            account: service.required_account(target).into(),
        })
        .collect(),
    }
}
struct Fixture {
    directory: tempfile::TempDir,
    host: Arc<Host>,
    config: Arc<Mutex<ProductConfigStore>>,
    effects: Arc<Effects>,
    request: p::ProductMaintenanceRequest,
}
impl Fixture {
    fn new(fault: &'static str) -> Self {
        let (directory, _, config) = fixture();
        config.lock().unwrap().apply(patch("model:one", 0)).unwrap();
        let root = directory.path();
        let binding = p::ProductDataBinding {
            schema_version: p::SchemaVersion(1),
            candidate_ref: "candidate.db".into(),
            store_path: root.join("brain/data-operations/candidate.db"),
            content_key_directory: root.join("brain/forme.content-keys"),
            owner_digest: p::sha256_content_digest(b"owner:test"),
            store_identity: digest("store"),
            manifest_digest: digest("candidate"),
            execution_fence_digest: Some(digest("fence")),
        };
        let effects = Arc::new(Effects {
            host: Mutex::new(Weak::new()),
            calls: Mutex::new(Vec::new()),
            state: Mutex::new(p::NativeServiceState::Running),
            fault,
            ack: root.join("config/ack.json"),
            admission: root.join("config/core.work-admission.json"),
            binding: binding.clone(),
        });
        let config = Arc::new(config);
        let host = Arc::new(
            ProductHostBroker::open_with_service_manager_and_config(
                &root.join("product.jsonl"),
                "install:config-test",
                HostCredentials::new(ADMIN.into(), None)
                    .unwrap()
                    .with_web_configuration(Some(WEB.into()))
                    .unwrap(),
                p::ProductTarget::WindowsX86_64,
                "0.1.0".into(),
                Some(config.clone()),
                Services(effects.clone()),
            )
            .unwrap()
            .with_restore_worker(Arc::new(Worker {
                install: installation(root),
                effects: effects.clone(),
            }))
            .unwrap(),
        );
        *effects.host.lock().unwrap() = Arc::downgrade(&host);
        let request = p::ProductMaintenanceRequest {
            schema_version: p::SchemaVersion(1),
            operation_id: OP.into(),
            expected_revision: 1,
            owner: p::OwnerPrincipalRef("owner:test".into()),
            requested_deadline: now() + 60_000,
            action: p::ProductMaintenanceAction::CommitRestore {
                candidate_ref: binding.candidate_ref,
                candidate_digest: binding.manifest_digest,
                owner_digest: binding.owner_digest,
                config_revision: 1,
            },
        };
        Self {
            directory,
            host,
            config,
            effects,
            request,
        }
    }
    fn execute(&self) -> p::MaintenanceOperation {
        let auth = self.host.authenticate_web_configuration(WEB).unwrap();
        self.host
            .owner_maintenance(&auth, self.request.clone(), now())
            .unwrap()
    }
}

#[test]
fn restore_stages_precede_effects_and_commit_releases_work_only_after_exact_readiness() {
    let f = Fixture::new("");
    assert_eq!(f.execute().stage, p::MaintenanceStage::Committed);
    assert_eq!(
        *f.effects.calls.lock().unwrap(),
        ["stop", "validate", "backup", "fence", "release", "start"]
    );
    let admission: p::ProductWorkAdmission =
        serde_json::from_slice(&fs::read(&f.effects.admission).unwrap()).unwrap();
    assert!(admission.released);
    for _ in 0..3 {
        assert_eq!(f.execute().stage, p::MaintenanceStage::Committed);
    }
    assert_eq!(f.effects.calls.lock().unwrap().len(), 6);
    // Lost release publication is reconciled from the committed operation,
    // without rerunning any service/worker effect.
    let mut held = admission;
    held.released = false;
    fs::write(&f.effects.admission, serde_json::to_vec(&held).unwrap()).unwrap();
    let auth = f.host.authenticate_web_configuration(WEB).unwrap();
    f.host.owner_maintenance_operation(&auth, OP).unwrap();
    let admission: p::ProductWorkAdmission =
        serde_json::from_slice(&fs::read(&f.effects.admission).unwrap()).unwrap();
    assert!(admission.released);
    assert_eq!(f.effects.calls.lock().unwrap().len(), 6);
    let core = f.directory.path().join("config/core.json");
    let before: serde_json::Value = serde_json::from_slice(&fs::read(&core).unwrap()).unwrap();
    f.config
        .lock()
        .unwrap()
        .apply(patch("model:two", 2))
        .unwrap();
    let after: serde_json::Value = serde_json::from_slice(&fs::read(&core).unwrap()).unwrap();
    assert_eq!(before["brain"]["store_path"], after["brain"]["store_path"]);
    assert_eq!(
        before["brain"]["content_key_directory"],
        after["brain"]["content_key_directory"]
    );
    assert_eq!(
        before["brain"]["expected_restoration_fence"],
        after["brain"]["expected_restoration_fence"]
    );
    assert_eq!(
        before["brain"]["models"][0]["profile"],
        after["brain"]["models"][0]["profile"]
    );
    let old = before["brain"]["models"][0]["credential_path"]
        .as_str()
        .unwrap();
    let new = after["brain"]["models"][0]["credential_path"]
        .as_str()
        .unwrap();
    assert_eq!(fs::read(old).unwrap(), fs::read(new).unwrap());
}

#[test]
fn every_interrupted_restore_keeps_original_operation_and_never_retries_side_effects() {
    for fault in [
        "stop",
        "unknown_stop",
        "validate",
        "foreign_candidate",
        "backup",
        "fence",
        "release",
        "start",
        "wrong_ack",
    ] {
        let f = Fixture::new(fault);
        let op = f.execute();
        assert_eq!(op.stage, p::MaintenanceStage::RecoveryRequired, "{fault}");
        assert!(op.pending_stage.is_some());
        let calls = f.effects.calls.lock().unwrap().clone();
        // Same operation queries its original facts; a different operation
        // cannot escape the unfinished installation transaction.
        assert_eq!(f.execute(), op);
        let auth = f.host.authenticate_web_configuration(WEB).unwrap();
        let mut next = f.request.clone();
        next.operation_id = "restore:second".into();
        next.expected_revision = op.revision;
        assert!(
            f.host.owner_maintenance(&auth, next, now()).is_err(),
            "{fault}"
        );
        assert_eq!(*f.effects.calls.lock().unwrap(), calls);
        if f.effects.admission.exists() {
            let held: p::ProductWorkAdmission =
                serde_json::from_slice(&fs::read(&f.effects.admission).unwrap()).unwrap();
            assert!(!held.released, "{fault}");
        }
    }
}

#[test]
fn stale_config_owner_and_expired_approval_do_not_stop_the_brain() {
    for mutation in 0..3 {
        let mut f = Fixture::new("");
        match mutation {
            0 => {
                if let p::ProductMaintenanceAction::CommitRestore {
                    config_revision, ..
                } = &mut f.request.action
                {
                    *config_revision = 2;
                }
            }
            1 => f.request.owner = p::OwnerPrincipalRef("owner:other".into()),
            _ => f.request.requested_deadline = now() - 1,
        }
        let auth = f.host.authenticate_web_configuration(WEB).unwrap();
        assert!(f.host.owner_maintenance(&auth, f.request, now()).is_err());
        assert!(f.effects.calls.lock().unwrap().is_empty());
    }
}
