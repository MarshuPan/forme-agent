//! M5 governed capability-ecosystem control plane.
#![forbid(unsafe_code)]

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use forme_capabilities::{
    CapabilityPackageRegistry, CapabilityPackageVerifier, Ed25519CapabilityPackageVerifier,
    InMemoryCapabilityRegistry, InMemoryPublisherKeyring, PublisherKeyring, PublisherPublicKey,
    StagedCapabilityPackage,
};
use forme_eval::RemoteGroundTruth;
use forme_execution::{RemoteClock, RemoteInnerDriver, RemoteInnerOutcome, ResolvedSecret};
use forme_protocol as p;
use forme_store::{
    EcosystemEventStore, EcosystemPackageArchive, EcosystemProjection, EcosystemRuntimeLedger,
    EventStore, SqliteEventStore,
};

const ECOSYSTEM_AGGREGATE: &str = "ecosystem";
const MAX_PUBLISHER_TTL_MS: i64 = 90 * 24 * 60 * 60 * 1_000;
pub const M5_PACKAGE_RECEIVER_LOGICAL_PATH: &str = "ecosystem/package";

/// Authority/executor local ledger for exact declarative package bytes. It is
/// deliberately independent from the authority EventStore: the receiver can
/// prove a durable local record without gaining event-writing authority.
pub struct FileCapabilityPackageLedger {
    root: PathBuf,
    lock: Mutex<()>,
}

impl FileCapabilityPackageLedger {
    pub fn open(root: impl AsRef<Path>) -> p::Result<Self> {
        fs::create_dir_all(root.as_ref())
            .map_err(|_| p::Error("executor package ledger directory is unavailable".into()))?;
        let root = fs::canonicalize(root.as_ref())
            .map_err(|_| p::Error("executor package ledger path is unavailable".into()))?;
        fs::create_dir_all(root.join("bundles"))
            .and_then(|_| fs::create_dir_all(root.join("records")))
            .map_err(|_| p::Error("executor package ledger layout is unavailable".into()))?;
        Ok(Self {
            root,
            lock: Mutex::new(()),
        })
    }

    pub fn install(
        &self,
        envelope: &p::CapabilityPackageDistributionEnvelope,
    ) -> p::Result<p::CapabilityExecutorInstallRecord> {
        envelope.validate()?;
        let _guard = self
            .lock
            .lock()
            .map_err(|_| p::Error("executor package ledger is unavailable".into()))?;
        let package_key = digest_key(&envelope.package.package_digest)?;
        let record_path = self
            .root
            .join("records")
            .join(format!("{package_key}.json"));
        if record_path.is_file() {
            let existing = read_record(&record_path)?;
            if existing.package != envelope.package.manifest.package
                || existing.release != envelope.package.manifest.release
                || existing.package_digest != envelope.package.package_digest
                || existing.envelope_digest != envelope.content_digest
            {
                return Err(p::Error(
                    "executor package identity has conflicting semantics".into(),
                ));
            }
            return Ok(existing);
        }

        let bundle_path = self
            .root
            .join("bundles")
            .join(format!("{package_key}.json"));
        let bundle = serde_json::to_vec(&envelope.package)
            .map_err(|_| p::Error("executor package bundle cannot be encoded".into()))?;
        if bundle_path.is_file() {
            let existing = fs::read(&bundle_path)
                .map_err(|_| p::Error("executor package bundle cannot be read".into()))?;
            let decoded = serde_json::from_slice::<p::SignedCapabilityPackage>(&existing)
                .map_err(|_| p::Error("executor package bundle is malformed".into()))?;
            decoded.validate()?;
            if decoded != envelope.package {
                return Err(p::Error(
                    "executor package bundle content address changed".into(),
                ));
            }
        } else {
            write_create_new(&bundle_path, &bundle)?;
        }

        let generation = self
            .read_records_locked()?
            .into_iter()
            .map(|record| record.installed_generation)
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or_else(|| p::Error("executor package generation is exhausted".into()))?;
        let mut record = p::CapabilityExecutorInstallRecord {
            schema_version: p::M5_SCHEMA_VERSION,
            peer: envelope.target_peer.clone(),
            package: envelope.package.manifest.package.clone(),
            release: envelope.package.manifest.release.clone(),
            package_digest: envelope.package.package_digest.clone(),
            envelope_digest: envelope.content_digest.clone(),
            installed_generation: generation,
            record_digest: p::SchemaDigest(String::new()),
        };
        record.refresh_digest()?;
        let bytes = serde_json::to_vec(&record)
            .map_err(|_| p::Error("executor package record cannot be encoded".into()))?;
        match write_create_new(&record_path, &bytes) {
            Ok(()) => Ok(record),
            Err(error) if record_path.is_file() => {
                let existing = read_record(&record_path)?;
                if existing.package == record.package
                    && existing.release == record.release
                    && existing.package_digest == record.package_digest
                    && existing.envelope_digest == record.envelope_digest
                {
                    Ok(existing)
                } else {
                    Err(error)
                }
            }
            Err(error) => Err(error),
        }
    }

    pub fn record_for_digest(
        &self,
        package_digest: &p::SchemaDigest,
    ) -> p::Result<Option<p::CapabilityExecutorInstallRecord>> {
        let _guard = self
            .lock
            .lock()
            .map_err(|_| p::Error("executor package ledger is unavailable".into()))?;
        let path = self
            .root
            .join("records")
            .join(format!("{}.json", digest_key(package_digest)?));
        if !path.is_file() {
            return Ok(None);
        }
        read_record(&path).map(Some)
    }

    pub fn record_count(&self) -> p::Result<usize> {
        let _guard = self
            .lock
            .lock()
            .map_err(|_| p::Error("executor package ledger is unavailable".into()))?;
        Ok(self.read_records_locked()?.len())
    }

    fn read_records_locked(&self) -> p::Result<Vec<p::CapabilityExecutorInstallRecord>> {
        let mut records = Vec::new();
        let entries = fs::read_dir(self.root.join("records"))
            .map_err(|_| p::Error("executor package ledger records are unavailable".into()))?;
        for entry in entries {
            let entry = entry
                .map_err(|_| p::Error("executor package ledger record is unreadable".into()))?;
            if !entry.path().is_file() {
                return Err(p::Error(
                    "executor package ledger contains an unexpected entry".into(),
                ));
            }
            records.push(read_record(&entry.path())?);
        }
        Ok(records)
    }
}

fn digest_key(digest: &p::SchemaDigest) -> p::Result<String> {
    let bytes = p::sha256_digest_bytes(digest)?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn write_create_new(path: &Path, bytes: &[u8]) -> p::Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|_| p::Error("executor package ledger record cannot be created".into()))?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|_| p::Error("executor package ledger record cannot be persisted".into()))
}

fn read_record(path: &Path) -> p::Result<p::CapabilityExecutorInstallRecord> {
    let bytes = fs::read(path)
        .map_err(|_| p::Error("executor package ledger record cannot be read".into()))?;
    let record: p::CapabilityExecutorInstallRecord = serde_json::from_slice(&bytes)
        .map_err(|_| p::Error("executor package ledger record is malformed".into()))?;
    record.validate()?;
    Ok(record)
}

pub struct CapabilityPackageReceiver {
    peer: p::FederatedPeerRef,
    grant: p::CapabilityPublisherGrant,
    policy: p::CapabilityAdmissionPolicy,
    authority_epoch: p::AuthorityEpoch,
    keyring: Arc<InMemoryPublisherKeyring>,
    ledger: Arc<FileCapabilityPackageLedger>,
    clock: Arc<dyn RemoteClock>,
}

impl CapabilityPackageReceiver {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        peer: p::FederatedPeerRef,
        grant: p::CapabilityPublisherGrant,
        policy: p::CapabilityAdmissionPolicy,
        authority_epoch: p::AuthorityEpoch,
        keyring: Arc<InMemoryPublisherKeyring>,
        ledger: Arc<FileCapabilityPackageLedger>,
        clock: Arc<dyn RemoteClock>,
    ) -> p::Result<Self> {
        grant.validate()?;
        policy.validate()?;
        if peer.0.trim().is_empty()
            || authority_epoch.0 == 0
            || grant.status != p::CapabilityPublisherStatus::Active
        {
            return Err(p::Error(
                "executor package receiver configuration is incomplete".into(),
            ));
        }
        Ok(Self {
            peer,
            grant,
            policy,
            authority_epoch,
            keyring,
            ledger,
            clock,
        })
    }

    pub fn ledger(&self) -> Arc<FileCapabilityPackageLedger> {
        self.ledger.clone()
    }

    fn receive(
        &self,
        operation: &p::RemoteOperation,
    ) -> p::Result<p::CapabilityExecutorInstallRecord> {
        let p::ActionParameters::File {
            operation: p::FileOperation::Write,
            path,
            content: Some(content),
        } = &operation.parameters
        else {
            return Err(p::Error(
                "executor package receiver requires one bounded file payload".into(),
            ));
        };
        if path != M5_PACKAGE_RECEIVER_LOGICAL_PATH
            || operation.capability.0 != "forme.ecosystem.package-receiver"
            || operation.scope != self.grant.scope
            || operation.expected_effect != p::ExpectedEffect::Outward
            || operation.action_type != p::ActionType::ExternalCommit
        {
            return Err(p::Error(
                "executor package receiver operation is outside its local policy".into(),
            ));
        }
        let envelope = serde_json::from_slice::<p::CapabilityPackageDistributionEnvelope>(content)
            .map_err(|_| p::Error("executor package envelope is malformed".into()))?;
        envelope.validate()?;
        if envelope.target_peer != self.peer
            || envelope.authority_epoch != self.authority_epoch
            || envelope.publisher_grant != self.grant
            || envelope.authority_policy != self.policy
        {
            return Err(p::Error(
                "executor package envelope does not match local authority bindings".into(),
            ));
        }
        for dependency in &envelope.dependency_admissions {
            let Some(record) = self.ledger.record_for_digest(&dependency.package_digest)? else {
                return Err(p::Error(
                    "executor package dependency is not installed locally".into(),
                ));
            };
            if record.package != dependency.package
                || record.release != dependency.release
                || record.package_digest != dependency.package_digest
            {
                return Err(p::Error(
                    "executor package dependency ledger binding changed".into(),
                ));
            }
        }
        let verified = Ed25519CapabilityPackageVerifier::new(self.keyring.clone()).verify(
            &envelope.package,
            &self.grant,
            &self.policy,
            &envelope.dependency_admissions,
            self.clock.now_ms(),
        )?;
        if verified.package != envelope.admission.package
            || verified.release != envelope.admission.release
            || verified.package_digest != envelope.admission.package_digest
            || verified.dependencies != envelope.admission.dependencies
        {
            return Err(p::Error(
                "executor local admission does not match authority package".into(),
            ));
        }
        self.ledger.install(&envelope)
    }
}

impl RemoteInnerDriver for CapabilityPackageReceiver {
    fn execute(
        &self,
        operation: &p::RemoteOperation,
        credential: Option<&ResolvedSecret>,
    ) -> p::Result<RemoteInnerOutcome> {
        if credential.is_some() {
            return Err(p::Error(
                "executor package receiver refuses central credentials".into(),
            ));
        }
        let record = self.receive(operation)?;
        Ok(RemoteInnerOutcome {
            outcome: p::RemoteReceiptOutcome::Completed,
            result_digest: Some(record.record_digest.clone()),
            observations: vec![p::EvidenceRef(format!(
                "executor-package-record:{}",
                record.record_digest.0
            ))],
        })
    }
}

/// Ground truth adapter used by the authority golden. It reads the
/// executor-local content-addressed record and never trusts the transport
/// status or worker self-report as the installed fact.
pub struct CapabilityPackageLedgerGroundTruth {
    ledger: Arc<FileCapabilityPackageLedger>,
}

impl CapabilityPackageLedgerGroundTruth {
    pub fn new(ledger: Arc<FileCapabilityPackageLedger>) -> Self {
        Self { ledger }
    }

    pub fn record_for_digest(
        &self,
        digest: &p::SchemaDigest,
    ) -> p::Result<Option<p::CapabilityExecutorInstallRecord>> {
        self.ledger.record_for_digest(digest)
    }
}

pub fn capability_distribution_operation(
    envelope: &p::CapabilityPackageDistributionEnvelope,
) -> p::Result<p::RemoteOperation> {
    envelope.validate()?;
    let content = serde_json::to_vec(envelope)
        .map_err(|_| p::Error("capability distribution envelope cannot be encoded".into()))?;
    let mut operation = p::RemoteOperation {
        schema_version: p::M5_SCHEMA_VERSION,
        backend: p::BackendKind::File,
        parameters: p::ActionParameters::File {
            operation: p::FileOperation::Write,
            path: M5_PACKAGE_RECEIVER_LOGICAL_PATH.into(),
            content: Some(content),
        },
        capability: p::CapabilityRef("forme.ecosystem.package-receiver".into()),
        scope: envelope.install_plan.scope.clone(),
        action_type: p::ActionType::ExternalCommit,
        expected_effect: p::ExpectedEffect::Outward,
        rollback_boundary: envelope.install_plan.rollback_boundary.clone(),
        credential_slot: None,
        digest: p::SchemaDigest(String::new()),
    };
    operation.refresh_digest()?;
    operation.validate()?;
    Ok(operation)
}

impl crate::m4::RemoteGroundTruthSource for CapabilityPackageLedgerGroundTruth {
    fn observe(
        &self,
        plan: &p::RemotePlacementPlan,
        lease: &p::RemoteExecutionLease,
        driver: &p::RemoteDriverReceipt,
    ) -> p::Result<Option<RemoteGroundTruth>> {
        let p::ActionParameters::File {
            operation: p::FileOperation::Write,
            content: Some(content),
            ..
        } = &plan.operation.parameters
        else {
            return Ok(None);
        };
        let envelope = serde_json::from_slice::<p::CapabilityPackageDistributionEnvelope>(content)
            .map_err(|_| p::Error("executor package ground truth envelope is malformed".into()))?;
        envelope.validate()?;
        let Some(record) = self
            .ledger
            .record_for_digest(&envelope.package.package_digest)?
        else {
            return Ok(None);
        };
        if record.peer != envelope.target_peer
            || record.package_digest != envelope.package.package_digest
            || driver.result_digest.as_ref() != Some(&record.record_digest)
            || driver.lease != lease.lease
            || driver.executor != plan.executor
        {
            return Ok(None);
        }
        Ok(Some(RemoteGroundTruth {
            schema_version: p::M4_SCHEMA_VERSION,
            outcome: p::RemoteReceiptOutcome::Completed,
            result_digest: Some(record.record_digest.clone()),
            evidence: vec![p::EvidenceRef(format!(
                "executor-ledger:{}:generation:{}",
                record.record_digest.0, record.installed_generation
            ))],
            verification: vec![p::EvidenceRef(format!(
                "executor-package-verified:{}",
                record.package_digest.0
            ))],
        }))
    }
}

pub trait EcosystemGatewayControl: Send + Sync {
    fn provision_capability_publisher(
        &self,
        run: p::RunId,
        grant: p::CapabilityPublisherGrant,
        previous: Option<p::CapabilityPublisherGrantRef>,
        expected: p::EcosystemAggregateVersion,
        owner: p::VerifiedPrincipal,
    ) -> p::Result<p::ExpectedAppend>;
    fn admit_capability_package(
        &self,
        run: p::RunId,
        catalog_run: p::RunId,
        package: p::SignedCapabilityPackage,
        now: p::Timestamp,
        owner: p::VerifiedPrincipal,
    ) -> p::Result<p::CapabilityPackageAdmission>;
    fn prepare_capability_change(
        &self,
        operation: p::CapabilityPackageOperation,
        package: p::CapabilityPackageRef,
        release: p::CapabilityReleaseRef,
        owner: p::VerifiedPrincipal,
    ) -> p::Result<p::CapabilityInstallPlan>;
    fn apply_capability_change(
        &self,
        run: p::RunId,
        plan: p::CapabilityInstallPlan,
        approval: p::CapabilityPackageApproval,
        reason: p::ReasonRef,
        now: p::Timestamp,
    ) -> p::Result<p::CapabilityPackageState>;
    fn prepare_capability_distribution(
        &self,
        plan: p::CapabilityInstallPlan,
        target_peer: p::FederatedPeerRef,
        authority_epoch: p::AuthorityEpoch,
        owner: p::VerifiedPrincipal,
        now: p::Timestamp,
    ) -> p::Result<p::CapabilityPackageDistributionEnvelope>;
    fn capability_ecosystem_snapshot(
        &self,
        scope: p::Scope,
    ) -> p::Result<p::CapabilityEcosystemSnapshot>;
}

pub struct EcosystemHarnessRuntime {
    store: SqliteEventStore,
    owner: p::VerifiedPrincipal,
    keyring: Arc<InMemoryPublisherKeyring>,
    verifier: Ed25519CapabilityPackageVerifier<InMemoryPublisherKeyring>,
    policy: p::CapabilityAdmissionPolicy,
    packages: Arc<CapabilityPackageRegistry>,
    control_lock: Mutex<()>,
    event_sequence: AtomicU64,
}

struct VerifiedDistributionInputs {
    package: p::SignedCapabilityPackage,
    admission: p::CapabilityPackageAdmission,
    dependencies: Vec<p::CapabilityPackageAdmission>,
    publisher: p::CapabilityPublisherGrant,
}

struct VerifiedRemoteDistribution<'a> {
    placement: &'a p::RemotePlacementPlan,
    lease: &'a p::RemoteExecutionLease,
    driver: &'a p::RemoteDriverReceipt,
    receipt: &'a p::RemoteExecutionReceipt,
}

pub struct CapabilityDistributionGuard {
    runtime: Arc<EcosystemHarnessRuntime>,
    plan: p::CapabilityInstallPlan,
    approval: p::CapabilityPackageApproval,
}

impl CapabilityDistributionGuard {
    pub fn new(
        runtime: Arc<EcosystemHarnessRuntime>,
        plan: p::CapabilityInstallPlan,
        approval: p::CapabilityPackageApproval,
    ) -> p::Result<Self> {
        plan.validate()?;
        approval.validate()?;
        if plan.operation != p::CapabilityPackageOperation::Distribute
            || approval.plan_digest != plan.digest
        {
            return Err(p::Error(
                "capability distribution guard is not bound to its approved plan".into(),
            ));
        }
        Ok(Self {
            runtime,
            plan,
            approval,
        })
    }
}

impl crate::m4::RemoteDispatchGuard for CapabilityDistributionGuard {
    fn recheck(
        &self,
        _run: &p::RunId,
        placement: &p::RemotePlacementPlan,
        now: p::Timestamp,
    ) -> p::Result<()> {
        self.runtime
            .authorize_distribution(&self.plan, &self.approval, placement, now)
    }
}

pub struct CapabilityDistributionObserver {
    runtime: Arc<EcosystemHarnessRuntime>,
    plan: p::CapabilityInstallPlan,
    ground_truth: Arc<CapabilityPackageLedgerGroundTruth>,
}

impl CapabilityDistributionObserver {
    pub fn new(
        runtime: Arc<EcosystemHarnessRuntime>,
        plan: p::CapabilityInstallPlan,
        ground_truth: Arc<CapabilityPackageLedgerGroundTruth>,
    ) -> p::Result<Self> {
        plan.validate()?;
        if plan.operation != p::CapabilityPackageOperation::Distribute {
            return Err(p::Error(
                "capability distribution observer requires a distribution plan".into(),
            ));
        }
        Ok(Self {
            runtime,
            plan,
            ground_truth,
        })
    }
}

impl crate::m4::RemoteCompletionObserver for CapabilityDistributionObserver {
    fn record_verified(
        &self,
        run: &p::RunId,
        placement: &p::RemotePlacementPlan,
        lease: &p::RemoteExecutionLease,
        driver: &p::RemoteDriverReceipt,
        receipt: &p::RemoteExecutionReceipt,
    ) -> p::Result<()> {
        self.runtime.record_verified_distribution(
            run,
            &self.plan,
            VerifiedRemoteDistribution {
                placement,
                lease,
                driver,
                receipt,
            },
            &self.ground_truth,
        )?;
        Ok(())
    }
}

impl EcosystemHarnessRuntime {
    pub fn local(store: SqliteEventStore) -> Self {
        let owner = std::env::var("FORME_OWNER_ID").unwrap_or_else(|_| "local-owner".into());
        Self::new(
            store,
            p::VerifiedPrincipal(owner),
            default_admission_policy(),
            Arc::new(InMemoryCapabilityRegistry::default()),
        )
        .expect("repository default ecosystem configuration is valid")
    }

    pub fn new(
        store: SqliteEventStore,
        owner: p::VerifiedPrincipal,
        policy: p::CapabilityAdmissionPolicy,
        registry: Arc<InMemoryCapabilityRegistry>,
    ) -> p::Result<Self> {
        Self::new_with_keyring(
            store,
            owner,
            policy,
            registry,
            Arc::new(InMemoryPublisherKeyring::default()),
        )
    }

    pub fn new_with_keyring(
        store: SqliteEventStore,
        owner: p::VerifiedPrincipal,
        policy: p::CapabilityAdmissionPolicy,
        registry: Arc<InMemoryCapabilityRegistry>,
        keyring: Arc<InMemoryPublisherKeyring>,
    ) -> p::Result<Self> {
        if owner.0.trim().is_empty() {
            return Err(p::Error("ecosystem configured owner is empty".into()));
        }
        policy.validate()?;
        let runtime = Self {
            store,
            owner,
            keyring: keyring.clone(),
            verifier: Ed25519CapabilityPackageVerifier::new(keyring),
            policy,
            packages: Arc::new(CapabilityPackageRegistry::new(registry)),
            control_lock: Mutex::new(()),
            event_sequence: AtomicU64::new(1),
        };
        runtime.rebuild_registry()?;
        Ok(runtime)
    }

    pub fn configure_publisher_key(
        &self,
        publisher: p::CapabilityPublisherRef,
        key: PublisherPublicKey,
    ) -> p::Result<()> {
        self.keyring.provision(publisher, key)
    }

    pub fn package_registry(&self) -> Arc<CapabilityPackageRegistry> {
        self.packages.clone()
    }

    pub fn snapshot(&self, scope: p::Scope) -> p::Result<p::CapabilityEcosystemSnapshot> {
        EcosystemProjection::snapshot(&self.store, scope)
    }

    pub fn provision_publisher(
        &self,
        run: p::RunId,
        grant: p::CapabilityPublisherGrant,
        previous: Option<p::CapabilityPublisherGrantRef>,
        expected: p::EcosystemAggregateVersion,
        owner: p::VerifiedPrincipal,
    ) -> p::Result<p::ExpectedAppend> {
        let _control = self.lock_control()?;
        self.require_owner(&owner)?;
        grant.validate()?;
        expected.validate()?;
        let now = current_time_ms()?;
        if grant.expires_at <= now || grant.expires_at.saturating_sub(now) > MAX_PUBLISHER_TTL_MS {
            return Err(p::Error(
                "publisher grant lifetime is outside policy".into(),
            ));
        }
        let key = self.keyring.public_key(&grant.publisher)?;
        if key.digest() != grant.public_key_digest {
            return Err(p::Error(
                "publisher key does not match the owner grant".into(),
            ));
        }
        self.require_expected(&expected)?;
        self.append_audit_start(&run, p::Source::OwnerControl, &owner)?;
        let event = self.event(
            run.clone(),
            p::EventPayload::CapabilityPublisherChanged(p::CapabilityPublisherChangedPayload {
                grant: grant.clone(),
                previous,
                committed_version: expected.next()?,
            }),
            owner_provenance(),
        );
        let outcome =
            self.store
                .append_ecosystem_expected(event, &ecosystem_aggregate(), expected)?;
        if outcome.status != p::ExpectedAppendStatus::Applied
            && outcome.status != p::ExpectedAppendStatus::Duplicate
        {
            return Err(p::Error("publisher mutation lost ecosystem CAS".into()));
        }
        if grant.status == p::CapabilityPublisherStatus::Revoked {
            self.fence_publisher_packages(&grant.publisher, &grant.scope)?;
        }
        self.append_run_complete(&run, p::Source::OwnerControl)?;
        Ok(outcome)
    }

    pub fn admit_package(
        &self,
        run: p::RunId,
        catalog_run: p::RunId,
        package: p::SignedCapabilityPackage,
        now: p::Timestamp,
        owner: p::VerifiedPrincipal,
    ) -> p::Result<p::CapabilityPackageAdmission> {
        let _control = self.lock_control()?;
        self.require_owner(&owner)?;
        package.validate()?;
        self.require_catalog_evidence(&catalog_run, &package)?;
        let grant = EcosystemProjection::publisher(&self.store, &package.manifest.publisher)?
            .ok_or_else(|| p::Error("package publisher is not provisioned".into()))?;
        let dependencies = package
            .manifest
            .dependencies
            .iter()
            .map(|dependency| {
                EcosystemProjection::admission(&self.store, &dependency.release)?
                    .ok_or_else(|| p::Error("package dependency has no authority admission".into()))
            })
            .collect::<p::Result<Vec<_>>>()?;
        let admission = self
            .verifier
            .verify(&package, &grant, &self.policy, &dependencies, now)?;
        self.store.archive_package(&package)?;
        self.packages.archive(package)?;
        let expected = self.store.ecosystem_version(&ecosystem_aggregate())?;
        self.append_audit_start(&run, p::Source::OwnerControl, &owner)?;
        let event = self.event(
            run.clone(),
            p::EventPayload::CapabilityPackageAdmitted(p::CapabilityPackageAdmittedPayload {
                admission: admission.clone(),
                committed_version: expected.next()?,
            }),
            verified_provenance(),
        );
        let outcome =
            self.store
                .append_ecosystem_expected(event, &ecosystem_aggregate(), expected)?;
        if outcome.status != p::ExpectedAppendStatus::Applied {
            return Err(p::Error("package admission lost ecosystem CAS".into()));
        }
        self.append_run_complete(&run, p::Source::OwnerControl)?;
        Ok(admission)
    }

    pub fn prepare_change(
        &self,
        operation: p::CapabilityPackageOperation,
        package: p::CapabilityPackageRef,
        release: p::CapabilityReleaseRef,
        owner: p::VerifiedPrincipal,
    ) -> p::Result<p::CapabilityInstallPlan> {
        let _control = self.lock_control()?;
        self.require_owner(&owner)?;
        let admission = EcosystemProjection::admission(&self.store, &release)?
            .ok_or_else(|| p::Error("package release is not admitted".into()))?;
        if admission.package != package {
            return Err(p::Error(
                "package release belongs to another package".into(),
            ));
        }
        let archived = self
            .archived_package(&release)?
            .ok_or_else(|| p::Error("admitted package body is unavailable".into()))?;
        if archived.package_digest != admission.package_digest
            || archived.manifest.package != package
        {
            return Err(p::Error("admitted package body binding changed".into()));
        }
        let grant = EcosystemProjection::publisher(&self.store, &archived.manifest.publisher)?
            .ok_or_else(|| p::Error("package publisher projection is unavailable".into()))?;
        if grant.reference != admission.publisher_grant
            || grant.version != admission.publisher_version
            || grant.status != p::CapabilityPublisherStatus::Active
            || grant.expires_at <= current_time_ms()?
        {
            return Err(p::Error("package publisher is inactive".into()));
        }
        self.reverify_admission(&archived, &admission, &grant, current_time_ms()?)?;
        let current = EcosystemProjection::package_state(&self.store, &package)?;
        validate_requested_operation(operation, current.as_ref(), &release)?;
        let expected = self.store.ecosystem_version(&ecosystem_aggregate())?;
        let previous_release = matches!(
            operation,
            p::CapabilityPackageOperation::Update | p::CapabilityPackageOperation::Rollback
        )
        .then(|| current.as_ref().map(|state| state.release.clone()))
        .flatten();
        let mut plan = p::CapabilityInstallPlan {
            schema_version: p::M5_SCHEMA_VERSION,
            reference: p::CapabilityInstallPlanRef(format!(
                "plan:{}:{}:{}:{}",
                operation_name(operation),
                package.0,
                release.0,
                expected.value
            )),
            operation,
            package,
            release,
            package_digest: admission.package_digest,
            admission: admission.reference,
            scope: archived.manifest.scope,
            policy: self.policy.reference.clone(),
            policy_version: self.policy.version,
            expected_version: expected,
            previous_release,
            rollback_boundary: p::RollbackBoundary(
                "registry visibility only; prior external effects remain historical".into(),
            ),
            digest: p::PlanDigest(String::new()),
        };
        plan.refresh_digest()?;
        plan.validate()?;
        Ok(plan)
    }

    pub fn prepare_distribution_envelope(
        &self,
        plan: p::CapabilityInstallPlan,
        target_peer: p::FederatedPeerRef,
        authority_epoch: p::AuthorityEpoch,
        owner: p::VerifiedPrincipal,
        now: p::Timestamp,
    ) -> p::Result<p::CapabilityPackageDistributionEnvelope> {
        let _control = self.lock_control()?;
        self.require_owner(&owner)?;
        let inputs = self.verified_distribution_inputs(&plan, now, true)?;
        if target_peer.0.trim().is_empty() || authority_epoch.0 == 0 {
            return Err(p::Error(
                "capability distribution target binding is incomplete".into(),
            ));
        }
        let mut envelope = p::CapabilityPackageDistributionEnvelope {
            schema_version: p::M5_SCHEMA_VERSION,
            install_plan: plan,
            package: inputs.package,
            admission: inputs.admission,
            dependency_admissions: inputs.dependencies,
            publisher_grant: inputs.publisher,
            authority_policy: self.policy.clone(),
            target_peer,
            authority_epoch,
            content_digest: p::SchemaDigest(String::new()),
        };
        envelope.refresh_digest()?;
        envelope.validate()?;
        Ok(envelope)
    }

    pub fn apply_change(
        &self,
        run: p::RunId,
        plan: p::CapabilityInstallPlan,
        approval: p::CapabilityPackageApproval,
        reason: p::ReasonRef,
        now: p::Timestamp,
    ) -> p::Result<p::CapabilityPackageState> {
        let _control = self.lock_control()?;
        plan.validate()?;
        approval.validate()?;
        self.require_owner(&approval.principal)?;
        if reason.0.trim().is_empty()
            || approval.expires_at <= now
            || approval.plan_digest != plan.digest
            || plan.policy != self.policy.reference
            || plan.policy_version != self.policy.version
        {
            return Err(p::Error(
                "package approval or policy binding is invalid".into(),
            ));
        }
        let actual = self.store.ecosystem_version(&ecosystem_aggregate())?;
        if actual != plan.expected_version {
            return Err(p::Error("package plan lost ecosystem snapshot CAS".into()));
        }
        let admission = EcosystemProjection::admission(&self.store, &plan.release)?
            .ok_or_else(|| p::Error("package plan admission disappeared".into()))?;
        if admission.reference != plan.admission
            || admission.package != plan.package
            || admission.package_digest != plan.package_digest
            || admission.policy != plan.policy
            || admission.policy_version != plan.policy_version
        {
            return Err(p::Error("package plan drifted after approval".into()));
        }
        let current = EcosystemProjection::package_state(&self.store, &plan.package)?;
        validate_requested_operation(plan.operation, current.as_ref(), &plan.release)?;
        let (from, to, generation) = lifecycle_change(plan.operation, current.as_ref())?;
        let archived = self
            .archived_package(&plan.release)?
            .ok_or_else(|| p::Error("approved package body is unavailable".into()))?;
        if archived.package_digest != plan.package_digest || archived.manifest.scope != plan.scope {
            return Err(p::Error("approved package body changed semantics".into()));
        }
        let publisher = EcosystemProjection::publisher(&self.store, &archived.manifest.publisher)?
            .ok_or_else(|| p::Error("package publisher projection is unavailable".into()))?;
        if publisher.reference != admission.publisher_grant
            || publisher.version != admission.publisher_version
            || publisher.status != p::CapabilityPublisherStatus::Active
            || publisher.expires_at <= now
        {
            return Err(p::Error("publisher changed after package approval".into()));
        }
        self.reverify_admission(&archived, &admission, &publisher, now)?;
        let staged = (to == p::CapabilityLifecycleState::Enabled)
            .then(|| self.packages.stage(&archived))
            .transpose()?;
        self.append_audit_start(&run, p::Source::OwnerControl, &approval.principal)?;
        self.append_approval(&run, &plan, &approval)?;
        if !self
            .store
            .claim_ecosystem_nonce(&approval.nonce, &plan.digest)?
        {
            return Err(p::Error(
                "package approval nonce was already consumed".into(),
            ));
        }
        let change = p::CapabilityPackageStateChange {
            schema_version: p::M5_SCHEMA_VERSION,
            plan: plan.reference.clone(),
            approval: approval.approval.clone(),
            package: plan.package.clone(),
            release: plan.release.clone(),
            from,
            to,
            active_generation: generation,
            reason,
            external_effects_reverted: false,
        };
        change.validate()?;
        let event = self.event(
            run.clone(),
            p::EventPayload::CapabilityPackageStateChanged(
                p::CapabilityPackageStateChangedPayload {
                    change,
                    committed_version: plan.expected_version.next()?,
                },
            ),
            owner_provenance(),
        );
        let outcome = self.store.append_ecosystem_expected(
            event,
            &ecosystem_aggregate(),
            plan.expected_version,
        )?;
        if outcome.status != p::ExpectedAppendStatus::Applied {
            return Err(p::Error("package lifecycle lost ecosystem CAS".into()));
        }
        self.apply_registry_visibility(&plan.package, to, generation, staged)?;
        self.append_run_complete(&run, p::Source::OwnerControl)?;
        EcosystemProjection::package_state(&self.store, &plan.package)?
            .ok_or_else(|| p::Error("committed package state is unavailable".into()))
    }

    fn apply_registry_visibility(
        &self,
        package: &p::CapabilityPackageRef,
        lifecycle: p::CapabilityLifecycleState,
        generation: u64,
        staged: Option<StagedCapabilityPackage>,
    ) -> p::Result<()> {
        match lifecycle {
            p::CapabilityLifecycleState::Enabled => self.packages.enable(
                staged.ok_or_else(|| p::Error("enabled package was not staged".into()))?,
                generation,
            ),
            p::CapabilityLifecycleState::Disabled | p::CapabilityLifecycleState::Revoked => {
                if self.packages.active(package)?.is_some() {
                    self.packages.disable(package, generation)?;
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn fence_publisher_packages(
        &self,
        publisher: &p::CapabilityPublisherRef,
        scope: &p::Scope,
    ) -> p::Result<()> {
        let snapshot = self.snapshot(scope.clone())?;
        for state in snapshot.packages {
            let archived = self
                .archived_package(&state.release)?
                .ok_or_else(|| p::Error("package archive is incomplete".into()))?;
            if archived.manifest.publisher == *publisher {
                self.packages.fence(&state.package)?;
            }
        }
        Ok(())
    }

    fn require_catalog_evidence(
        &self,
        run: &p::RunId,
        package: &p::SignedCapabilityPackage,
    ) -> p::Result<()> {
        let events = self
            .store
            .read_run(run.clone())
            .collect::<p::Result<Vec<_>>>()?;
        let plans = events
            .iter()
            .enumerate()
            .filter_map(|(index, event)| match &event.payload {
                p::EventPayload::ActionPlanned(payload) => Some((index, event, payload)),
                _ => None,
            })
            .collect::<Vec<_>>();
        let requested = events
            .iter()
            .enumerate()
            .filter_map(|(index, event)| match &event.payload {
                p::EventPayload::ApprovalRequested(payload) => Some((index, payload)),
                _ => None,
            })
            .collect::<Vec<_>>();
        let resolved = events
            .iter()
            .enumerate()
            .filter_map(|(index, event)| match &event.payload {
                p::EventPayload::ApprovalResolved(payload) => Some((index, payload)),
                _ => None,
            })
            .collect::<Vec<_>>();
        let started = events
            .iter()
            .enumerate()
            .filter_map(|(index, event)| match &event.payload {
                p::EventPayload::ActionStarted(payload) => Some((index, payload)),
                _ => None,
            })
            .collect::<Vec<_>>();
        let outputs = events
            .iter()
            .enumerate()
            .filter_map(|(index, event)| match &event.payload {
                p::EventPayload::ActionOutputDelta(payload) => Some((index, event, payload)),
                _ => None,
            })
            .collect::<Vec<_>>();
        let completed = events
            .iter()
            .enumerate()
            .filter_map(|(index, event)| match &event.payload {
                p::EventPayload::ActionCompleted(payload) => Some((index, event, payload)),
                _ => None,
            })
            .collect::<Vec<_>>();
        let verification_started = events
            .iter()
            .enumerate()
            .filter_map(|(index, event)| match &event.payload {
                p::EventPayload::VerificationStarted(payload) => Some((index, event, payload)),
                _ => None,
            })
            .collect::<Vec<_>>();
        let verification_finished = events
            .iter()
            .enumerate()
            .filter_map(|(index, event)| match &event.payload {
                p::EventPayload::VerificationFinished(payload) => Some((index, event, payload)),
                _ => None,
            })
            .collect::<Vec<_>>();
        let run_completed = events
            .iter()
            .enumerate()
            .filter_map(|(index, event)| match event.payload {
                p::EventPayload::RunComplete(_) => Some(index),
                _ => None,
            })
            .collect::<Vec<_>>();
        let failed_action = events.iter().any(|event| {
            matches!(
                event.payload,
                p::EventPayload::ActionFailed(_)
                    | p::EventPayload::ActionDenied(_)
                    | p::EventPayload::ActionCancelled(_)
                    | p::EventPayload::ActionOutcomeUnknown(_)
                    | p::EventPayload::RunAborted(_)
                    | p::EventPayload::RunFailed(_)
                    | p::EventPayload::RunLimited(_)
            )
        });
        if plans.len() != 1
            || requested.len() != 1
            || resolved.len() != 1
            || started.len() != 1
            || outputs.is_empty()
            || completed.len() != 1
            || verification_started.len() != 1
            || verification_finished.len() != 1
            || run_completed.len() != 1
            || failed_action
        {
            return Err(p::Error(
                "catalog run is not one complete governed action".into(),
            ));
        }

        let (planned_at, planned_event, plan) = plans[0];
        let (requested_at, request) = requested[0];
        let (resolved_at, resolution) = resolved[0];
        let (started_at, start) = started[0];
        let (completed_at, completed_event, completion) = completed[0];
        let (verification_started_at, verification_start_event, verification_start) =
            verification_started[0];
        let (verification_finished_at, verification_finish_event, verification_finish) =
            verification_finished[0];
        let run_completed_at = run_completed[0];
        let approval = plan
            .approval_ref
            .as_ref()
            .ok_or_else(|| p::Error("catalog action has no bound approval".into()))?;
        let done_contract = p::DoneContractRef(format!("done-contract:{}", run.0));
        let first_output_at = outputs[0].0;
        let last_output_at = outputs
            .last()
            .map(|(index, _, _)| *index)
            .unwrap_or(first_output_at);
        let ordered = requested_at < resolved_at
            && resolved_at < planned_at
            && planned_at < started_at
            && started_at < first_output_at
            && last_output_at < completed_at
            && completed_at < verification_started_at
            && verification_started_at < verification_finished_at
            && verification_finished_at < run_completed_at;
        let binding_ok = plan.backend == p::BackendKind::AppApi
            && plan.expected_effect == p::ExpectedEffect::Outward
            && plan.scope == package.manifest.scope
            && plan.remote_placement.is_none()
            && planned_event.provenance.trust_tier == p::TrustTier::VerifiedProcess
            && request.approval_id == *approval
            && request.risk == p::Risk::High
            && request.scope == plan.scope
            && resolution.approval_id == *approval
            && resolution.outcome == p::ApprovalOutcome::Granted
            && resolution.grant_ref.is_some()
            && start.intent_id == plan.intent_id
            && start.backend == p::BackendKind::AppApi
            && start.scope == plan.scope
            && start.remote_lease.is_none();

        let mut body = String::new();
        let output_ok = outputs.iter().all(|(_, event, output)| {
            let valid = output.intent_id == plan.intent_id
                && output.backend == p::BackendKind::AppApi
                && output.scope == plan.scope
                && output.trust == p::TrustTier::Untrusted
                && !output.truncated
                && output.remote_lease.is_none()
                && event.provenance.trust_tier == p::TrustTier::Untrusted;
            body.push_str(&output.delta);
            valid
        });
        let body_digest = p::sha256_content_digest(body.as_bytes());
        let receipt_ok = completion.intent_id == plan.intent_id
            && completion.remote_receipt.is_none()
            && completed_event.provenance.trust_tier == p::TrustTier::Untrusted
            && completion.receipt.as_ref().is_some_and(|receipt| {
                receipt.schema_version.0 > 0
                    && receipt.action == plan.intent_id
                    && receipt.content_ref.is_some()
                    && receipt.content_digest.as_ref() == Some(&body_digest)
                    && receipt.trust == p::TrustTier::Untrusted
                    && receipt.effect == p::EffectStatus::Observed
                    && receipt.probe_hint.is_none()
            });
        let verification_ok = verification_start.against == done_contract
            && verification_finish.against == done_contract
            && verification_start.verifier_kind == verification_finish.verifier_kind
            && verification_finish.outcome == p::VerificationOutcome::Pass
            && verification_start_event.provenance.trust_tier == p::TrustTier::VerifiedProcess
            && verification_finish_event.provenance.trust_tier == p::TrustTier::VerifiedProcess;
        let observed = serde_json::from_str::<p::SignedCapabilityPackage>(&body)
            .map_err(|_| p::Error("catalog response is not a closed capability package".into()))?;
        observed.validate()?;
        if !ordered
            || !binding_ok
            || !output_ok
            || !receipt_ok
            || !verification_ok
            || observed != *package
        {
            return Err(p::Error(
                "package admission lacks a governed catalog receipt and verification".into(),
            ));
        }
        Ok(())
    }

    fn archived_package(
        &self,
        release: &p::CapabilityReleaseRef,
    ) -> p::Result<Option<p::SignedCapabilityPackage>> {
        if let Some(package) = self.packages.archived(release)? {
            return Ok(Some(package));
        }
        let package = self.store.archived_package(release)?;
        if let Some(package) = &package {
            self.packages.archive(package.clone())?;
        }
        Ok(package)
    }

    fn rebuild_registry(&self) -> p::Result<()> {
        let now = current_time_ms()?;
        for state in self.store.enabled_package_states()? {
            let package = self.archived_package(&state.release)?.ok_or_else(|| {
                p::Error("enabled package body is unavailable after restart".into())
            })?;
            let admission = EcosystemProjection::admission(&self.store, &state.release)?
                .ok_or_else(|| p::Error("enabled package admission is unavailable".into()))?;
            let publisher =
                EcosystemProjection::publisher(&self.store, &package.manifest.publisher)?
                    .ok_or_else(|| p::Error("enabled package publisher is unavailable".into()))?;
            if publisher.status == p::CapabilityPublisherStatus::Revoked {
                continue;
            }
            if admission.package != state.package
                || admission.package_digest != state.package_digest
                || package.package_digest != state.package_digest
                || publisher.reference != admission.publisher_grant
                || publisher.version != admission.publisher_version
                || admission.policy != self.policy.reference
                || admission.policy_version != self.policy.version
            {
                return Err(p::Error(
                    "enabled package cannot be rebuilt from its authority bindings".into(),
                ));
            }
            self.reverify_admission(&package, &admission, &publisher, now)?;
            let staged = self.packages.stage(&package)?;
            self.packages.enable(staged, state.active_generation)?;
        }
        Ok(())
    }

    fn verified_distribution_inputs(
        &self,
        plan: &p::CapabilityInstallPlan,
        now: p::Timestamp,
        require_version: bool,
    ) -> p::Result<VerifiedDistributionInputs> {
        plan.validate()?;
        if plan.operation != p::CapabilityPackageOperation::Distribute
            || plan.previous_release.is_some()
            || plan.policy != self.policy.reference
            || plan.policy_version != self.policy.version
        {
            return Err(p::Error(
                "capability distribution plan is not bound to current policy".into(),
            ));
        }
        if require_version
            && self.store.ecosystem_version(&ecosystem_aggregate())? != plan.expected_version
        {
            return Err(p::Error(
                "capability distribution plan lost ecosystem snapshot CAS".into(),
            ));
        }
        let admission = EcosystemProjection::admission(&self.store, &plan.release)?
            .ok_or_else(|| p::Error("distributed package admission is unavailable".into()))?;
        let package = self
            .archived_package(&plan.release)?
            .ok_or_else(|| p::Error("distributed package body is unavailable".into()))?;
        if admission.reference != plan.admission
            || admission.package != plan.package
            || admission.package_digest != plan.package_digest
            || package.manifest.package != plan.package
            || package.manifest.release != plan.release
            || package.package_digest != plan.package_digest
            || package.manifest.scope != plan.scope
        {
            return Err(p::Error(
                "capability distribution package binding changed".into(),
            ));
        }
        if EcosystemProjection::package_state(&self.store, &plan.package)?
            .is_some_and(|state| state.lifecycle == p::CapabilityLifecycleState::Revoked)
        {
            return Err(p::Error("revoked package cannot be distributed".into()));
        }
        let publisher =
            EcosystemProjection::publisher(&self.store, &package.manifest.publisher)?
                .ok_or_else(|| p::Error("distributed package publisher is unavailable".into()))?;
        if publisher.reference != admission.publisher_grant
            || publisher.version != admission.publisher_version
            || publisher.status != p::CapabilityPublisherStatus::Active
            || publisher.expires_at <= now
        {
            return Err(p::Error("distributed package publisher is inactive".into()));
        }
        self.reverify_admission(&package, &admission, &publisher, now)?;
        let mut dependencies = package
            .manifest
            .dependencies
            .iter()
            .map(|dependency| {
                EcosystemProjection::admission(&self.store, &dependency.release)?
                    .ok_or_else(|| p::Error("distributed dependency is unavailable".into()))
            })
            .collect::<p::Result<Vec<_>>>()?;
        dependencies.sort_by(|left, right| left.release.cmp(&right.release));
        Ok(VerifiedDistributionInputs {
            package,
            admission,
            dependencies,
            publisher,
        })
    }

    fn authorize_distribution(
        &self,
        plan: &p::CapabilityInstallPlan,
        approval: &p::CapabilityPackageApproval,
        placement: &p::RemotePlacementPlan,
        now: p::Timestamp,
    ) -> p::Result<()> {
        let _control = self.lock_control()?;
        approval.validate()?;
        self.require_owner(&approval.principal)?;
        if approval.expires_at <= now || approval.plan_digest != plan.digest {
            return Err(p::Error(
                "capability distribution approval is invalid or expired".into(),
            ));
        }
        let inputs = self.verified_distribution_inputs(plan, now, true)?;
        let envelope = distribution_envelope_from_placement(placement)?;
        if envelope.install_plan != *plan
            || envelope.package != inputs.package
            || envelope.admission != inputs.admission
            || envelope.dependency_admissions != inputs.dependencies
            || envelope.publisher_grant != inputs.publisher
            || envelope.authority_policy != self.policy
            || envelope.target_peer != placement.executor
            || envelope.authority_epoch != placement.authority_epoch
        {
            return Err(p::Error(
                "remote distribution operation drifted after owner approval".into(),
            ));
        }
        if !self
            .store
            .claim_ecosystem_nonce(&approval.nonce, &plan.digest)?
        {
            return Err(p::Error(
                "capability distribution approval nonce was already consumed".into(),
            ));
        }
        Ok(())
    }

    fn record_verified_distribution(
        &self,
        run: &p::RunId,
        plan: &p::CapabilityInstallPlan,
        completion: VerifiedRemoteDistribution<'_>,
        ground_truth: &CapabilityPackageLedgerGroundTruth,
    ) -> p::Result<p::CapabilityPackageDistributionReceipt> {
        let _control = self.lock_control()?;
        let VerifiedRemoteDistribution {
            placement,
            lease,
            driver,
            receipt: remote,
        } = completion;
        let envelope = distribution_envelope_from_placement(placement)?;
        if envelope.install_plan != *plan
            || envelope.target_peer != placement.executor
            || envelope.authority_epoch != placement.authority_epoch
        {
            return Err(p::Error(
                "verified distribution no longer matches its package plan".into(),
            ));
        }
        let record = ground_truth
            .record_for_digest(&plan.package_digest)?
            .ok_or_else(|| {
                p::Error("verified distribution has no executor ledger record".into())
            })?;
        remote.validate()?;
        driver.validate()?;
        lease.validate()?;
        if remote.outcome != p::RemoteReceiptOutcome::Completed
            || remote.driver_receipt != driver.receipt
            || remote.driver_receipt_digest != p::canonical_digest(driver)?
            || remote.lease != lease.lease
            || remote.plan_digest != lease.plan_digest
            || remote.operation_digest != placement.operation.digest
            || remote.executor != placement.executor
            || record.peer != placement.executor
            || record.package != plan.package
            || record.release != plan.release
            || record.package_digest != plan.package_digest
            || record.envelope_digest != envelope.content_digest
            || driver.result_digest.as_ref() != Some(&record.record_digest)
        {
            return Err(p::Error(
                "verified distribution receipt disagrees with executor ground truth".into(),
            ));
        }
        let identity = p::canonical_digest(&(
            &run,
            &plan.digest,
            &remote.receipt,
            &record.record_digest,
            &lease.lease,
        ))?;
        let receipt = p::CapabilityPackageDistributionReceipt {
            schema_version: p::M5_SCHEMA_VERSION,
            reference: p::CapabilityDistributionReceiptRef(format!("distribution:{}", identity.0)),
            package: plan.package.clone(),
            release: plan.release.clone(),
            package_digest: plan.package_digest.clone(),
            peer: placement.executor.clone(),
            peer_grant: placement.peer_grant.clone(),
            authority_epoch: placement.authority_epoch,
            plan_digest: lease.plan_digest.clone(),
            lease: lease.lease.clone(),
            fence_token: lease.fence.0,
            installed_generation: record.installed_generation,
            ground_truth: p::EvidenceRef(format!(
                "executor-package-record:{}",
                record.record_digest.0
            )),
            verified: p::RequiredTrue,
        };
        receipt.validate()?;
        let snapshot = self.snapshot(plan.scope.clone())?;
        if snapshot.distributions.contains(&receipt.reference) {
            let existing = self
                .store
                .distribution_receipt(&receipt.reference)?
                .ok_or_else(|| p::Error("distribution fact lost its durable attempt".into()))?;
            if existing != receipt {
                return Err(p::Error(
                    "distribution receipt identity changed semantics".into(),
                ));
            }
            return Ok(existing);
        }
        self.verified_distribution_inputs(plan, current_time_ms()?, true)?;
        self.store.record_distribution_attempt(&receipt)?;
        let event = self.event(
            run.clone(),
            p::EventPayload::CapabilityPackageDistributionRecorded(
                p::CapabilityPackageDistributionRecordedPayload {
                    receipt: receipt.clone(),
                    committed_version: plan.expected_version.next()?,
                },
            ),
            verified_provenance(),
        );
        let outcome = self.store.append_ecosystem_expected(
            event,
            &ecosystem_aggregate(),
            plan.expected_version.clone(),
        )?;
        if outcome.status != p::ExpectedAppendStatus::Applied {
            return Err(p::Error(
                "verified package distribution lost ecosystem CAS".into(),
            ));
        }
        Ok(receipt)
    }

    fn reverify_admission(
        &self,
        package: &p::SignedCapabilityPackage,
        admission: &p::CapabilityPackageAdmission,
        publisher: &p::CapabilityPublisherGrant,
        now: p::Timestamp,
    ) -> p::Result<()> {
        let dependencies = package
            .manifest
            .dependencies
            .iter()
            .map(|dependency| {
                EcosystemProjection::admission(&self.store, &dependency.release)?
                    .ok_or_else(|| p::Error("package dependency admission is unavailable".into()))
            })
            .collect::<p::Result<Vec<_>>>()?;
        let mut verified =
            self.verifier
                .verify(package, publisher, &self.policy, &dependencies, now)?;
        verified.admitted_at = admission.admitted_at;
        if &verified != admission {
            return Err(p::Error(
                "package admission no longer matches verified authority inputs".into(),
            ));
        }
        Ok(())
    }

    fn append_audit_start(
        &self,
        run: &p::RunId,
        source: p::Source,
        owner: &p::VerifiedPrincipal,
    ) -> p::Result<()> {
        self.append(
            run.clone(),
            p::EventPayload::RunAccepted(p::RunAcceptedPayload {
                source,
                session_ref: p::SessionId(format!("session:{}", run.0)),
                input_ref: p::InputRef(format!("ecosystem-control:{}", run.0)),
                idempotency_key: None,
            }),
            owner_provenance(),
        )?;
        self.append(
            run.clone(),
            p::EventPayload::SessionBound(p::SessionBoundPayload {
                policy_profile: p::PolicyProfileRef(self.policy.reference.0.clone()),
                model_profile: p::ModelProfileRef("model:none".into()),
                toolset_ref: p::ToolsetRef("toolset:ecosystem-control".into()),
                workspace: p::WorkspaceRef("workspace:ecosystem-control".into()),
                effect_mode: None,
                evolution_snapshot: None,
                federation_snapshot: None,
            }),
            p::Provenance {
                source,
                actor: p::Actor::Owner,
                trust_tier: p::TrustTier::OwnerInput,
                caused_by: Some(p::EventId(format!("principal:{}", owner.0))),
            },
        )?;
        Ok(())
    }

    fn append_approval(
        &self,
        run: &p::RunId,
        plan: &p::CapabilityInstallPlan,
        approval: &p::CapabilityPackageApproval,
    ) -> p::Result<()> {
        let archived = self
            .archived_package(&plan.release)?
            .ok_or_else(|| p::Error("approved package body is unavailable".into()))?;
        let mut permissions = archived
            .manifest
            .contributions
            .iter()
            .flat_map(|contribution| contribution.required_permissions.iter().cloned())
            .collect::<Vec<_>>();
        permissions.sort();
        permissions.dedup();
        self.append(
            run.clone(),
            p::EventPayload::ApprovalRequested(p::ApprovalRequestedPayload {
                approval_id: approval.approval.clone(),
                action_summary: p::ActionSummary(format!(
                    "{} capability package {}",
                    operation_name(plan.operation),
                    plan.package.0
                )),
                risk: p::Risk::High,
                scope: plan.scope.clone(),
                rollback_boundary: plan.rollback_boundary.clone(),
                expires_at: approval.expires_at,
                choices: vec![
                    p::ApprovalChoice("approve once".into()),
                    p::ApprovalChoice("deny".into()),
                ],
                requested_permissions: permissions,
                affected_resources: vec![p::ResourceRef(plan.package.0.clone())],
            }),
            owner_provenance(),
        )?;
        self.append(
            run.clone(),
            p::EventPayload::ApprovalResolved(p::ApprovalResolvedPayload {
                approval_id: approval.approval.clone(),
                outcome: p::ApprovalOutcome::Granted,
                grant_ref: Some(p::ApprovalGrantRef(format!(
                    "ecosystem:{}",
                    approval.approval.0
                ))),
            }),
            owner_provenance(),
        )?;
        Ok(())
    }

    fn append_run_complete(&self, run: &p::RunId, source: p::Source) -> p::Result<()> {
        self.append(
            run.clone(),
            p::EventPayload::RunComplete(p::RunCompletePayload {
                stop_reason: p::StopReason("ecosystem control committed".into()),
                result_ref: None,
            }),
            p::Provenance {
                source,
                actor: p::Actor::System,
                trust_tier: p::TrustTier::VerifiedProcess,
                caused_by: None,
            },
        )?;
        Ok(())
    }

    fn append(
        &self,
        run: p::RunId,
        payload: p::EventPayload,
        provenance: p::Provenance,
    ) -> p::Result<p::EventId> {
        self.store.append(self.event(run, payload, provenance))
    }

    fn event(
        &self,
        run: p::RunId,
        payload: p::EventPayload,
        provenance: p::Provenance,
    ) -> p::Event {
        let sequence = self.event_sequence.fetch_add(1, Ordering::SeqCst);
        p::Event::new(
            p::EventId(format!("event:m5:{}:{sequence}", run.0)),
            run,
            None,
            payload,
            p::M5_SCHEMA_VERSION,
            current_time_ms().unwrap_or(1),
            provenance,
        )
    }

    fn require_owner(&self, owner: &p::VerifiedPrincipal) -> p::Result<()> {
        if owner != &self.owner {
            return Err(p::Error(
                "ecosystem control requires the configured owner".into(),
            ));
        }
        Ok(())
    }

    fn require_expected(&self, expected: &p::EcosystemAggregateVersion) -> p::Result<()> {
        if self.store.ecosystem_version(&ecosystem_aggregate())? != *expected {
            return Err(p::Error("ecosystem expected version is stale".into()));
        }
        Ok(())
    }

    fn lock_control(&self) -> p::Result<std::sync::MutexGuard<'_, ()>> {
        self.control_lock
            .lock()
            .map_err(|_| p::Error("ecosystem owner control is unavailable".into()))
    }
}

impl EcosystemGatewayControl for EcosystemHarnessRuntime {
    fn provision_capability_publisher(
        &self,
        run: p::RunId,
        grant: p::CapabilityPublisherGrant,
        previous: Option<p::CapabilityPublisherGrantRef>,
        expected: p::EcosystemAggregateVersion,
        owner: p::VerifiedPrincipal,
    ) -> p::Result<p::ExpectedAppend> {
        self.provision_publisher(run, grant, previous, expected, owner)
    }

    fn admit_capability_package(
        &self,
        run: p::RunId,
        catalog_run: p::RunId,
        package: p::SignedCapabilityPackage,
        now: p::Timestamp,
        owner: p::VerifiedPrincipal,
    ) -> p::Result<p::CapabilityPackageAdmission> {
        self.admit_package(run, catalog_run, package, now, owner)
    }

    fn prepare_capability_change(
        &self,
        operation: p::CapabilityPackageOperation,
        package: p::CapabilityPackageRef,
        release: p::CapabilityReleaseRef,
        owner: p::VerifiedPrincipal,
    ) -> p::Result<p::CapabilityInstallPlan> {
        self.prepare_change(operation, package, release, owner)
    }

    fn apply_capability_change(
        &self,
        run: p::RunId,
        plan: p::CapabilityInstallPlan,
        approval: p::CapabilityPackageApproval,
        reason: p::ReasonRef,
        now: p::Timestamp,
    ) -> p::Result<p::CapabilityPackageState> {
        self.apply_change(run, plan, approval, reason, now)
    }

    fn prepare_capability_distribution(
        &self,
        plan: p::CapabilityInstallPlan,
        target_peer: p::FederatedPeerRef,
        authority_epoch: p::AuthorityEpoch,
        owner: p::VerifiedPrincipal,
        now: p::Timestamp,
    ) -> p::Result<p::CapabilityPackageDistributionEnvelope> {
        self.prepare_distribution_envelope(plan, target_peer, authority_epoch, owner, now)
    }

    fn capability_ecosystem_snapshot(
        &self,
        scope: p::Scope,
    ) -> p::Result<p::CapabilityEcosystemSnapshot> {
        self.snapshot(scope)
    }
}

impl EcosystemGatewayControl for crate::ReactiveHarness {
    fn provision_capability_publisher(
        &self,
        run: p::RunId,
        grant: p::CapabilityPublisherGrant,
        previous: Option<p::CapabilityPublisherGrantRef>,
        expected: p::EcosystemAggregateVersion,
        owner: p::VerifiedPrincipal,
    ) -> p::Result<p::ExpectedAppend> {
        self.ecosystem
            .provision_publisher(run, grant, previous, expected, owner)
    }

    fn admit_capability_package(
        &self,
        run: p::RunId,
        catalog_run: p::RunId,
        package: p::SignedCapabilityPackage,
        now: p::Timestamp,
        owner: p::VerifiedPrincipal,
    ) -> p::Result<p::CapabilityPackageAdmission> {
        self.ecosystem
            .admit_package(run, catalog_run, package, now, owner)
    }

    fn prepare_capability_change(
        &self,
        operation: p::CapabilityPackageOperation,
        package: p::CapabilityPackageRef,
        release: p::CapabilityReleaseRef,
        owner: p::VerifiedPrincipal,
    ) -> p::Result<p::CapabilityInstallPlan> {
        self.ecosystem
            .prepare_change(operation, package, release, owner)
    }

    fn apply_capability_change(
        &self,
        run: p::RunId,
        plan: p::CapabilityInstallPlan,
        approval: p::CapabilityPackageApproval,
        reason: p::ReasonRef,
        now: p::Timestamp,
    ) -> p::Result<p::CapabilityPackageState> {
        self.ecosystem
            .apply_change(run, plan, approval, reason, now)
    }

    fn prepare_capability_distribution(
        &self,
        plan: p::CapabilityInstallPlan,
        target_peer: p::FederatedPeerRef,
        authority_epoch: p::AuthorityEpoch,
        owner: p::VerifiedPrincipal,
        now: p::Timestamp,
    ) -> p::Result<p::CapabilityPackageDistributionEnvelope> {
        self.ecosystem
            .prepare_distribution_envelope(plan, target_peer, authority_epoch, owner, now)
    }

    fn capability_ecosystem_snapshot(
        &self,
        scope: p::Scope,
    ) -> p::Result<p::CapabilityEcosystemSnapshot> {
        self.ecosystem.snapshot(scope)
    }
}

fn validate_requested_operation(
    operation: p::CapabilityPackageOperation,
    current: Option<&p::CapabilityPackageState>,
    release: &p::CapabilityReleaseRef,
) -> p::Result<()> {
    let valid = match operation {
        p::CapabilityPackageOperation::Install => current.is_none(),
        p::CapabilityPackageOperation::Enable => current.is_some_and(|state| {
            state.release == *release
                && matches!(
                    state.lifecycle,
                    p::CapabilityLifecycleState::Installed | p::CapabilityLifecycleState::Disabled
                )
        }),
        p::CapabilityPackageOperation::Disable => current.is_some_and(|state| {
            state.release == *release && state.lifecycle == p::CapabilityLifecycleState::Enabled
        }),
        p::CapabilityPackageOperation::Update | p::CapabilityPackageOperation::Rollback => current
            .is_some_and(|state| {
                state.lifecycle == p::CapabilityLifecycleState::Enabled && state.release != *release
            }),
        p::CapabilityPackageOperation::Revoke => current.is_none_or(|state| {
            state.release == *release && state.lifecycle != p::CapabilityLifecycleState::Revoked
        }),
        p::CapabilityPackageOperation::Distribute => {
            current.is_none_or(|state| state.lifecycle != p::CapabilityLifecycleState::Revoked)
        }
    };
    if !valid {
        return Err(p::Error(
            "requested package lifecycle operation is not valid from current state".into(),
        ));
    }
    Ok(())
}

fn lifecycle_change(
    operation: p::CapabilityPackageOperation,
    current: Option<&p::CapabilityPackageState>,
) -> p::Result<(
    p::CapabilityLifecycleState,
    p::CapabilityLifecycleState,
    u64,
)> {
    let (from, generation) = current
        .map(|state| (state.lifecycle, state.active_generation))
        .unwrap_or((p::CapabilityLifecycleState::Admitted, 0));
    let to = match operation {
        p::CapabilityPackageOperation::Install => p::CapabilityLifecycleState::Installed,
        p::CapabilityPackageOperation::Enable
        | p::CapabilityPackageOperation::Update
        | p::CapabilityPackageOperation::Rollback => p::CapabilityLifecycleState::Enabled,
        p::CapabilityPackageOperation::Disable => p::CapabilityLifecycleState::Disabled,
        p::CapabilityPackageOperation::Revoke => p::CapabilityLifecycleState::Revoked,
        p::CapabilityPackageOperation::Distribute => {
            return Err(p::Error("distribution is not a lifecycle mutation".into()))
        }
    };
    Ok((
        from,
        to,
        generation
            .checked_add(1)
            .ok_or_else(|| p::Error("package generation is exhausted".into()))?,
    ))
}

fn distribution_envelope_from_placement(
    placement: &p::RemotePlacementPlan,
) -> p::Result<p::CapabilityPackageDistributionEnvelope> {
    placement.validate()?;
    let p::ActionParameters::File {
        operation: p::FileOperation::Write,
        path,
        content: Some(content),
    } = &placement.operation.parameters
    else {
        return Err(p::Error(
            "remote distribution operation is not a bounded package payload".into(),
        ));
    };
    if placement.operation.backend != p::BackendKind::File
        || path != M5_PACKAGE_RECEIVER_LOGICAL_PATH
        || placement.operation.capability.0 != "forme.ecosystem.package-receiver"
        || placement.operation.action_type != p::ActionType::ExternalCommit
        || placement.operation.expected_effect != p::ExpectedEffect::Outward
        || placement.operation.credential_slot.is_some()
    {
        return Err(p::Error(
            "remote distribution operation is outside the package receiver boundary".into(),
        ));
    }
    let envelope = serde_json::from_slice::<p::CapabilityPackageDistributionEnvelope>(content)
        .map_err(|_| p::Error("remote distribution package envelope is malformed".into()))?;
    envelope.validate()?;
    if envelope.target_peer != placement.executor
        || envelope.authority_epoch != placement.authority_epoch
        || envelope.install_plan.scope != placement.operation.scope
        || envelope.install_plan.rollback_boundary != placement.operation.rollback_boundary
    {
        return Err(p::Error(
            "remote distribution placement does not match its package envelope".into(),
        ));
    }
    Ok(envelope)
}

fn ecosystem_aggregate() -> p::EcosystemAggregateRef {
    p::EcosystemAggregateRef(ECOSYSTEM_AGGREGATE.into())
}

fn operation_name(operation: p::CapabilityPackageOperation) -> &'static str {
    match operation {
        p::CapabilityPackageOperation::Install => "install",
        p::CapabilityPackageOperation::Enable => "enable",
        p::CapabilityPackageOperation::Disable => "disable",
        p::CapabilityPackageOperation::Update => "update",
        p::CapabilityPackageOperation::Rollback => "rollback",
        p::CapabilityPackageOperation::Revoke => "revoke",
        p::CapabilityPackageOperation::Distribute => "distribute",
    }
}

fn default_admission_policy() -> p::CapabilityAdmissionPolicy {
    p::CapabilityAdmissionPolicy {
        schema_version: p::M5_SCHEMA_VERSION,
        reference: p::CapabilityPolicyRef("policy:ecosystem-default".into()),
        version: p::Version(1),
        allowed_kinds: vec![
            p::CapabilityPackageKind::Connector,
            p::CapabilityPackageKind::Plugin,
            p::CapabilityPackageKind::Skill,
            p::CapabilityPackageKind::McpServer,
            p::CapabilityPackageKind::AgentProfile,
        ],
        allowed_licenses: vec!["Apache-2.0".into(), "MIT".into()],
        max_package_bytes: 1_048_576,
        max_dependencies: 32,
        max_depth: 8,
        allow_network: false,
        allow_hooks: false,
    }
}

fn owner_provenance() -> p::Provenance {
    p::Provenance {
        source: p::Source::OwnerControl,
        actor: p::Actor::Owner,
        trust_tier: p::TrustTier::OwnerInput,
        caused_by: None,
    }
}

fn verified_provenance() -> p::Provenance {
    p::Provenance {
        source: p::Source::Internal,
        actor: p::Actor::System,
        trust_tier: p::TrustTier::VerifiedProcess,
        caused_by: None,
    }
}

fn current_time_ms() -> p::Result<i64> {
    use std::time::{SystemTime, UNIX_EPOCH};
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| p::Error("system clock is before the Unix epoch".into()))?
        .as_millis();
    i64::try_from(millis).map_err(|_| p::Error("system clock exceeds timestamp range".into()))
}
