use super::*;
use forme_store::{
    AuthorityLeaseGuard, BrainAuthorityClaim, BrainAuthorityStore, ContentKeyResolver,
    FileContentKeyResolver, StoreOptions,
};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
};

pub(super) fn now() -> i64 {
    crate::now_ms()
}
pub(super) fn owner() -> p::VerifiedPrincipal {
    p::VerifiedPrincipal("owner:worker-material".into())
}
pub(super) fn scope() -> p::Scope {
    p::Scope("workspace:worker-material".into())
}

pub(super) struct Directory(pub PathBuf);
impl Directory {
    pub fn new() -> Arc<Self> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "forme-worker-material-{}-{}-{}",
            std::process::id(),
            now(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir(&path).unwrap();
        Arc::new(Self(path))
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

pub(super) struct FaultKeys {
    inner: FileContentKeyResolver,
    pub allocations: AtomicUsize,
    pub fail_at: AtomicUsize,
    pub fail_destroy: AtomicBool,
}
impl ContentKeyResolver for FaultKeys {
    fn resolve(&self, id: &p::SchemaDigest) -> p::Result<Vec<u8>> {
        self.inner.resolve(id)
    }
    fn destroy(&self, id: &p::SchemaDigest) -> p::Result<()> {
        if self.fail_destroy.load(Ordering::SeqCst) {
            return Err(p::Error("fixture key deletion interruption".into()));
        }
        self.inner.destroy(id)
    }
    fn subject_key(
        &self,
        binding: &p::ContentBinding,
        subject: &p::SchemaDigest,
    ) -> p::Result<p::SchemaDigest> {
        let call = self.allocations.fetch_add(1, Ordering::SeqCst) + 1;
        if call == self.fail_at.load(Ordering::SeqCst) {
            return Err(p::Error("fixture interrupted protected write".into()));
        }
        self.inner.subject_key(binding, subject)
    }
}
pub(super) struct Fixture {
    pub sink: Arc<ProtectedRuntimeArtifacts>,
    pub store: SqliteEventStore,
    pub protection: StoreContentProtection,
    pub keys: Arc<FaultKeys>,
    _guard: AuthorityLeaseGuard,
    pub root: Arc<Directory>,
}
impl Fixture {
    pub fn new() -> Self {
        Self::open(Directory::new())
    }
    pub fn open(root: Arc<Directory>) -> Self {
        let store =
            SqliteEventStore::open(root.0.join("brain.db"), StoreOptions::default()).unwrap();
        let keys = Arc::new(FaultKeys {
            inner: FileContentKeyResolver::new(root.0.join("keys")).unwrap(),
            allocations: AtomicUsize::new(0),
            fail_at: AtomicUsize::new(0),
            fail_destroy: AtomicBool::new(false),
        });
        let protection = StoreContentProtection::new(&owner(), scope(), keys.clone()).unwrap();
        store.enable_content_protection(protection.clone()).unwrap();
        let authority = store
            .claim_brain_authority(BrainAuthorityClaim {
                owner_digest: p::sha256_content_digest(owner().0.as_bytes()),
                process_nonce: p::Nonce(format!("worker-material:{}", now())),
                now_ms: now(),
                lease_ttl: p::DurationMs(60_000),
            })
            .unwrap();
        let guard = store
            .install_authority_lease_guard(&authority, p::DurationMs(60_000))
            .unwrap();
        let sink = Arc::new(ProtectedRuntimeArtifacts {
            inner: FileArtifactStore::new(root.0.join("artifacts")).unwrap(),
            protection: protection.clone(),
            store: store.clone(),
            owner: owner(),
        });
        Self {
            sink,
            store,
            protection,
            keys,
            _guard: guard,
            root,
        }
    }
    pub fn body(&self, id: &p::ContentRef) -> Option<ProtectedContentRecord> {
        self.store.protected_content_for_current_owner(id).unwrap()
    }
    pub fn put(&self, object: &str, bytes: &[u8]) -> ProtectedContentRecord {
        let (_, record) = self
            .protection
            .seal_record(
                bytes,
                &scope(),
                object,
                &p::sha256_content_digest(scope().0.as_bytes()),
            )
            .unwrap();
        self.protection
            .lifecycle_service(self.store.clone())
            .put_protected_content(&owner(), record.clone())
            .unwrap();
        record
    }
    pub fn deletion(&self) -> forme_store::DataLifecycleRequest {
        forme_store::DataLifecycleRequest {
            schema_version: p::V1_CLOSURE_SCHEMA_VERSION,
            owner: owner(),
            scope: scope(),
            subject_digest: p::sha256_content_digest(scope().0.as_bytes()),
            operation: p::DataLifecycleOperation::CryptoShred,
            expected_version: 0,
            event_id: p::EventId("event:delete-worker-material".into()),
            timestamp: now(),
            remote_disposition: p::RemoteDeletionDisposition::Unknown,
            evidence: vec![p::EvidenceRef("evidence:worker-material-test".into())],
        }
    }
}

pub(super) fn intent() -> p::ActionIntent {
    p::ActionIntent {
        schema_version: p::SchemaVersion(1),
        intent_id: p::ActionId("action:worker-material".into()),
        source: p::Source::Internal,
        goal: p::GoalRef("goal:worker-material".into()),
        backend_hint: p::BackendKind::Computer,
        capability_ref: p::CapabilityRef("computer:worker-material".into()),
        action_type: p::ActionType::Execute,
        scope: scope(),
        risk_hint: p::Risk::High,
        expected_effect: p::ExpectedEffect::Internal,
        rollback_expectation: p::RollbackBoundary("none".into()),
        requested_permissions: vec![],
        requested_at: now(),
        estimated_output_bytes: 4096,
        estimated_duration: p::DurationMs(1000),
        parameters: p::ActionParameters::Computer(p::ComputerActionSpec {
            schema_version: p::SchemaVersion(1),
            driver: p::ProviderId("driver:worker-material".into()),
            surface: p::SurfaceRef("surface:worker-material".into()),
            bounds: p::CoordinateBounds {
                schema_version: p::SchemaVersion(1),
                min_x: 0,
                min_y: 0,
                max_x_exclusive: 2,
                max_y_exclusive: 1,
            },
            operation: p::ComputerOperation::Screenshot,
            artifact_scope: scope(),
        }),
    }
}
pub(super) fn material() -> p::WorkerArtifactBinding {
    let at = now();
    let digest = p::sha256_content_digest(&png());
    let exchange = p::DesktopExchangeBinding {
        schema_version: p::DESKTOP_OUTPUT_SCHEMA_VERSION,
        installation_ref: "installation:worker-material".into(),
        owner_ref: owner(),
        job_id: "job:worker-material".into(),
        permit_ref: "permit:worker-material".into(),
        permit_digest: p::sha256_content_digest(b"original fixture permit"),
        action_ref: intent().intent_id,
        workspace_scope: scope(),
        plan_digest: p::PlanDigest(p::sha256_content_digest(b"original fixture plan").0),
        normalized_action_digest: p::normalized_action_digest(&intent()).unwrap(),
        authority_epoch: p::AuthorityEpoch(1),
        nonce: p::Nonce("nonce:worker-material".into()),
        grant_ref: "grant:worker-material".into(),
        os_session_ref: "os-session:worker-material".into(),
        session_generation: 1,
        deadline: at + 60_000,
    };
    let binding = p::WorkerResultBinding {
        schema_version: p::SchemaVersion(1),
        installation_ref: exchange.installation_ref.clone(),
        job_id: exchange.job_id.clone(),
        owner_ref: owner(),
        action_ref: exchange.action_ref.clone(),
        workspace_scope: scope(),
        plan_digest: exchange.plan_digest.clone(),
        normalized_action_digest: Some(exchange.normalized_action_digest.clone()),
        permit_ref: exchange.permit_ref.clone(),
        permit_digest: exchange.permit_digest.clone(),
        authority_epoch: exchange.authority_epoch,
        nonce: exchange.nonce.clone(),
        receipt_ref: Some("receipt:worker-material".into()),
        output_budget_bytes: 4096,
    };
    let mut manifest = p::WorkerResultManifest {
        schema_version: p::SchemaVersion(1),
        manifest_digest: p::SchemaDigest(String::new()),
        binding,
        result_ref: Some("result:worker-material".into()),
        status: p::WorkerResultStatus::Available,
        outcome: Some(p::WorkerOutcome::Completed),
        bytes: png().len() as u64,
        content_digest: Some(digest.clone()),
        encoding: p::WorkerResultEncoding::Binary,
        truncated: false,
        produced_at: at,
        evidence_refs: vec![],
    };
    manifest.manifest_digest = manifest.expected_digest().unwrap();
    let descriptor = p::DesktopOutputDescriptor {
        schema_version: p::DESKTOP_OUTPUT_SCHEMA_VERSION,
        binding: exchange,
        manifest_digest: manifest.manifest_digest.clone(),
        content_digest: digest,
        bytes: manifest.bytes,
        media_type: "image/png".into(),
        width: 2,
        height: 1,
    };
    let material = p::WorkerArtifactBinding {
        schema_version: p::DESKTOP_OUTPUT_SCHEMA_VERSION,
        manifest,
        descriptor,
    };
    material.validate().unwrap();
    material
}

// Self-authored two-pixel RGBA PNG, generated from one filter-0 row.
pub(super) fn png() -> Vec<u8> {
    vec![
        137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 2, 0, 0, 0, 1, 8, 6,
        0, 0, 0, 244, 34, 127, 138, 0, 0, 0, 17, 73, 68, 65, 84, 120, 156, 99, 144, 48, 240, 248,
        159, 80, 49, 225, 63, 0, 13, 203, 3, 247, 20, 148, 106, 176, 0, 0, 0, 0, 73, 69, 78, 68,
        174, 66, 96, 130,
    ]
}
