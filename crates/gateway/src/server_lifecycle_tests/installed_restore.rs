//! Actual encrypted SQLite/key/session worker; native service installation is separate.
use super::*;
use crate::{
    offline_data::{OfflineCoreMaintenance, OfflineRestoreWorker},
    web::{
        identity::{
            SensitiveLoginRequest, SensitivePassword, SessionCookie, WebIdentityService,
            WebIdentityStore, WebRequestBinding,
        },
        WebServerConfig,
    },
};
use ed25519_dalek::{Signer, SigningKey};
use std::path::Path;

const INSTALL: &str = "install:restore-worker";
const PASSWORD: &str = "local restore fixture password 92";
fn digest(value: &str) -> p::SchemaDigest {
    p::canonical_digest(&value).unwrap()
}
fn json(path: &Path, value: &impl serde::Serialize) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, serde_json::to_vec(value).unwrap()).unwrap();
}
fn installation(root: &Path) -> p::InstallationManifest {
    let target = p::ProductTarget::WindowsX86_64;
    p::InstallationManifest {
        schema_version: p::SchemaVersion(1),
        install_id: INSTALL.into(),
        target,
        release_id: "0.1.0".into(),
        release_digest: digest("release"),
        created_at: now_ms(),
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
pub(super) struct RestoreFixture {
    pub(super) f: Fixture,
    pub(super) manifest: PathBuf,
    pub(super) core: PathBuf,
    pub(super) request: p::ProductRestoreRequest,
    identity: WebIdentityStore,
    session: SessionCookie,
    template: p::ProductConfigTemplate,
}
impl RestoreFixture {
    pub(super) fn new() -> Self {
        let mut f = Fixture::new();
        let install = installation(&f.root);
        f.config.store_path = install.paths.brain.join("forme.db");
        f.config.data.artifact_root = install.paths.brain.join("artifacts");
        drop(f.build());
        for path in [
            &install.paths.backups,
            &install.paths.host_state,
            &install.paths.config,
        ] {
            fs::create_dir_all(path).unwrap();
        }
        let signing = SigningKey::from_bytes(&[117; 32]);
        let template = p::ProductConfigTemplate {
            schema_version: p::SchemaVersion(1),
            install_ref: INSTALL.into(),
            core_config_path: install.paths.config.join("core/core.json"),
            configuration_ack_path: install.paths.brain.join("runtime/core-config-ack.json"),
            model_credentials_directory: install.paths.keys.join("model-credentials"),
            brain_store_path: f.config.store_path.clone(),
            artifact_root: f.config.data.artifact_root.clone(),
            core_owner_credential_path: install.paths.config.join("core/owner.token"),
            core_web_credential_path: install.paths.config.join("core/web.token"),
            core_web_assertion_verifying_key: signing.verifying_key().to_bytes(),
            core_session_fence_path: install.paths.brain.join("runtime/core-auth.sqlite"),
            core_bind: "127.0.0.1:8770".parse().unwrap(),
            web_bind: "127.0.0.1:8765".parse().unwrap(),
            web_network_config_path: None,
            web_material_directory: None,
            workspace: f.config.workspace.clone(),
            agent_profile: f.config.agent_profile.clone(),
            scheduler_tick: p::DurationMs(1000),
            scheduler_lease: p::DurationMs(30_000),
            scheduler_max_claims: 1,
            cognition_tick: p::DurationMs(60_000),
            stable_evidence_max_age: p::DurationMs(86_400_000),
            authority_lease: p::DurationMs(30_000),
            drain_timeout: p::DurationMs(30_000),
        };
        let manifest = install.paths.config.join("installation.json");
        json(&manifest, &install);
        json(
            &install.paths.config.join("product-template.json"),
            &template,
        );
        let core = template.core_config_path.clone();
        // Offline recovery intentionally does not decode model/transport runtime inputs.
        json(
            &core,
            &serde_json::json!({ "schema_version":1, "install_ref":INSTALL, "revision":1,
            "brain": {"schema_version":1,"owner":f.config.owner,"workspace":f.config.workspace,
                "store_path":f.config.store_path,"artifact_root":f.config.data.artifact_root,
                "agent_profile":null,"models":null,"scheduler_enabled":null,"scheduler":null,"cognition":null,"authority_lease":null},
            "transport":null,"drain_timeout_ms":null }),
        );
        let source = OfflineCoreMaintenance::open(&core, None).unwrap();
        let backup = install.paths.backups.join("source.db");
        source.backup(&backup).unwrap();
        drop(source);
        let source = OfflineCoreMaintenance::open(&core, Some(&backup)).unwrap();
        let candidate = install.paths.brain.join("data-operations/candidate.db");
        fs::create_dir_all(candidate.parent().unwrap()).unwrap();
        let plan = source.plan_restore(&candidate).unwrap();
        source.restore(&candidate, &plan, &plan.digest).unwrap();
        drop(source);
        let candidate_source = OfflineCoreMaintenance::open(&core, Some(&candidate)).unwrap();
        let candidate_digest =
            p::canonical_digest(&candidate_source.verify().unwrap().manifest).unwrap();
        drop(candidate_source);
        let web = WebServerConfig {
            network_config_path: None,
            network_material_directory: None,
            schema_version: p::SchemaVersion(1),
            install_ref: INSTALL.into(),
            identity_directory: install.paths.web_auth.clone(),
            claim_verifying_key: signing.verifying_key().to_bytes(),
            bind: "127.0.0.1:8765".parse().unwrap(),
            tls: None,
            bootstrap_bridge: None,
            configuration_bridge: None,
            core_bridge: None,
        };
        json(&install.paths.config.join("web.json"), &web);
        fs::create_dir_all(&web.identity_directory).unwrap();
        let identity =
            WebIdentityStore::open(&web.identity_directory, INSTALL, signing.verifying_key())
                .unwrap();
        let commitment = identity
            .prepare_claim(
                "claim:restore",
                "fixture",
                SensitivePassword::new(PASSWORD.into()),
            )
            .unwrap();
        let receipt = p::WebClaimCommit {
            schema_version: p::SchemaVersion(1),
            install_ref: INSTALL.into(),
            operation_id: "claim:restore".into(),
            candidate_ref: "data:restore".into(),
            owner: f.config.owner.clone(),
            account_commitment: commitment,
            expires_at: now_ms() + 60_000,
        };
        let signed = p::canonical_digest(&("forme-web-claim-commit-v1", &receipt)).unwrap();
        identity
            .commit_claim(&p::SignedWebClaimCommit {
                schema_version: p::SchemaVersion(1),
                receipt,
                signature: signing.sign(signed.0.as_bytes()).to_bytes().to_vec(),
            })
            .unwrap();
        // The core account creates its original fence files before stopping.
        let core_bridge = super::super::web_bridge::WebBridgeState::open(
            Some(INSTALL.into()),
            Some(
                super::super::web_bridge::LocalBearerSecret::new(
                    "fixture-web-bridge-abcdefghijklmnopqrstuvwxyz".into(),
                )
                .unwrap(),
            ),
            Some(signing.verifying_key()),
            Some(&template.core_session_fence_path),
            &p::VerifiedPrincipal(f.config.owner.0.clone()),
        )
        .unwrap();
        drop(core_bridge);
        let session = identity
            .login(SensitiveLoginRequest {
                login_name: "fixture".into(),
                password: SensitivePassword::new(PASSWORD.into()),
            })
            .unwrap();
        let request = p::ProductRestoreRequest {
            schema_version: p::SchemaVersion(1),
            install_ref: INSTALL.into(),
            operation_id: "restore:worker".into(),
            command_digest: digest("owner-approved-restore-command"),
            expected_config_revision: 1,
            owner: f.config.owner.clone(),
            candidate_ref: "candidate.db".into(),
            candidate_digest,
        };
        Self {
            f,
            manifest,
            core,
            request,
            identity,
            session,
            template,
        }
    }
    fn step(&self, step: p::ProductRestoreStep) -> p::ProductRestoreStepRequest {
        p::ProductRestoreStepRequest {
            schema_version: p::SchemaVersion(1),
            operation_id: self.request.operation_id.clone(),
            command_digest: self.request.command_digest.clone(),
            step,
        }
    }
}

#[test]
fn installed_restore_worker_holds_both_real_databases_backs_up_and_fences_sessions() {
    let f = RestoreFixture::new();
    let before = fs::read(&f.f.config.store_path).unwrap();
    let old_epoch = f.session.view.auth_epoch;
    let mut worker = OfflineRestoreWorker::open(&f.manifest, f.request.clone()).unwrap();
    worker
        .validated()
        .unwrap()
        .validate_for(&f.request)
        .unwrap();
    assert!(OfflineCoreMaintenance::open(&f.core, None).is_err());
    let candidate = f.f.root.join("brain/data-operations/candidate.db");
    assert!(OfflineCoreMaintenance::open(&f.core, Some(&candidate)).is_err());
    assert!(worker.step(f.step(p::ProductRestoreStep::Release)).is_err());
    assert!(worker
        .step(f.step(p::ProductRestoreStep::FenceSessions))
        .is_err());
    let backup = worker.step(f.step(p::ProductRestoreStep::Backup)).unwrap();
    let p::ProductRestoreWorkerResult::BackedUp {
        backup_ref,
        manifest_digest,
    } = backup.result
    else {
        panic!("wrong result")
    };
    let backed_up = f.f.root.join("backups").join(backup_ref);
    let source = OfflineCoreMaintenance::open(&f.core, Some(&backed_up)).unwrap();
    assert_eq!(
        p::canonical_digest(&source.verify().unwrap().manifest).unwrap(),
        manifest_digest
    );
    drop(source);
    assert!(worker.step(f.step(p::ProductRestoreStep::Backup)).is_err());
    let fenced = worker
        .step(f.step(p::ProductRestoreStep::FenceSessions))
        .unwrap();
    let p::ProductRestoreWorkerResult::SessionsFenced { revision } = fenced.result else {
        panic!("wrong result")
    };
    assert_eq!(revision.auth_epoch, old_epoch + 1);
    assert!(f
        .identity
        .authenticate(
            &f.session.cookie,
            WebRequestBinding {
                mutation: false,
                csrf: None
            }
        )
        .is_err());
    let fresh = f
        .identity
        .login(SensitiveLoginRequest {
            login_name: "fixture".into(),
            password: SensitivePassword::new(PASSWORD.into()),
        })
        .unwrap();
    assert_eq!(fresh.view.owner, f.request.owner);
    assert_eq!(revision.credential_revision, 1);
    assert_eq!(fresh.view.auth_epoch, revision.auth_epoch);
    assert_eq!(
        f.identity
            .fence_for_local_maintenance(
                &f.request.owner,
                &f.request.operation_id,
                &f.request.command_digest
            )
            .unwrap(),
        revision
    );
    assert!(f
        .identity
        .fence_for_local_maintenance(
            &f.request.owner,
            &f.request.operation_id,
            &digest("different-command")
        )
        .is_err());
    assert!(f.template.core_session_fence_path.is_file());
    worker.step(f.step(p::ProductRestoreStep::Release)).unwrap();
    drop(worker);
    assert!(OfflineCoreMaintenance::open(&f.core, None).is_ok());
    assert_eq!(fs::read(&f.f.config.store_path).unwrap(), before);
    assert_eq!(f.f.calls.load(Ordering::SeqCst), 0);
}

#[test]
fn installed_restore_worker_rejects_stale_revision_foreign_owner_digest_and_cross_operation_steps()
{
    let f = RestoreFixture::new();
    for mutation in 0..4 {
        let mut request = f.request.clone();
        match mutation {
            0 => request.expected_config_revision += 1,
            1 => request.owner = p::OwnerPrincipalRef("owner:foreign".into()),
            2 => request.candidate_digest = digest("different-candidate"),
            _ => request.candidate_ref = "../candidate.db".into(),
        }
        assert!(OfflineRestoreWorker::open(&f.manifest, request).is_err());
    }
    let mut worker = OfflineRestoreWorker::open(&f.manifest, f.request.clone()).unwrap();
    let mut step = f.step(p::ProductRestoreStep::Backup);
    step.operation_id = "restore:foreign".into();
    assert!(worker.step(step).is_err());
    assert!(!f.f.root.join("backups").read_dir().unwrap().any(|e| e
        .unwrap()
        .file_name()
        .to_string_lossy()
        .starts_with("restore-")));
    assert_eq!(f.f.calls.load(Ordering::SeqCst), 0);
}

#[test]
fn managed_work_admission_rechecks_each_claim_and_cannot_use_foreign_or_missing_release() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("work-admission.json");
    let identity = p::ProductDataAcknowledgement {
        schema_version: p::SchemaVersion(1),
        owner_digest: digest("owner"),
        store_identity: digest("store"),
        execution_fence_digest: Some(digest("fence")),
        composition_digest: digest("composition"),
    };
    let admission = Arc::new(super::super::maintenance_admission::managed_admission(
        path.clone(),
        INSTALL.into(),
        2,
        identity.clone(),
    ));
    assert!(admission.acquire().is_err());
    let mut record = p::ProductWorkAdmission {
        schema_version: p::SchemaVersion(1),
        install_ref: INSTALL.into(),
        operation_id: "restore:work".into(),
        command_digest: digest("command"),
        config_revision: 2,
        owner_digest: identity.owner_digest,
        execution_fence_digest: digest("fence"),
        released: false,
    };
    json(&path, &record);
    assert!(admission.acquire().is_err());
    record.released = true;
    json(&path, &record);
    drop(admission.acquire().unwrap());
    for mutation in 0..5 {
        let mut bad = record.clone();
        match mutation {
            0 => bad.install_ref = "install:foreign".into(),
            1 => bad.owner_digest = digest("foreign-owner"),
            2 => bad.execution_fence_digest = digest("foreign-fence"),
            3 => bad.config_revision = 3,
            _ => bad.schema_version = p::SchemaVersion(2),
        }
        json(&path, &bad);
        assert!(admission.acquire().is_err());
    }
    fs::write(&path, b"torn record").unwrap();
    assert!(admission.acquire().is_err());
}
