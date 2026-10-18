//! Owner data controls on the shared production runtime. Destructive commands
//! bind a persisted approval request, one-shot nonce, exact plan and version.
#![forbid(unsafe_code)]
use crate::{now_ms, ProductionRuntime};
use forme_protocol as p;
use forme_store::{ApprovalConsumptionClaim, ApprovalConsumptionLedger, EventStore};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
static DATA_PLAN_NONCE: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DataOperationPlan {
    pub schema_version: p::SchemaVersion,
    pub owner_digest: p::SchemaDigest,
    pub operation: DataPlannedOperation,
    pub expires_at: p::Timestamp,
    pub nonce: p::Nonce,
    pub digest: p::SchemaDigest,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum DataPlannedOperation {
    Restore {
        source: String,
        target: String,
        backup_digest: p::SchemaDigest,
    },
    Lifecycle {
        plan: forme_store::DataLifecyclePlan,
    },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DataOperationConfirmation {
    pub schema_version: p::SchemaVersion,
    pub plan_digest: p::SchemaDigest,
    pub nonce: p::Nonce,
    pub use_by: p::Timestamp,
    pub approved: bool,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum DataOperationResult {
    Restored(forme_store::RestoreReport),
    Lifecycle(p::DataLifecycleReceipt),
}
impl DataOperationPlan {
    fn refresh(&mut self) -> p::Result<()> {
        self.digest = p::SchemaDigest(String::new());
        self.digest = p::canonical_digest(self)?;
        Ok(())
    }
    fn validate(&self) -> p::Result<()> {
        let mut p = self.clone();
        p.refresh()?;
        if self.schema_version.0 != 1 || p.digest != self.digest || self.nonce.0.is_empty() {
            Err(p::Error("data operation plan binding is invalid".into()))
        } else {
            Ok(())
        }
    }
    fn run(&self) -> p::RunId {
        p::RunId(format!("data-control:{}", self.digest.0))
    }
    fn approval(&self) -> p::ApprovalId {
        p::ApprovalId(format!("data-approval:{}", self.digest.0))
    }
    fn resources(&self) -> Vec<p::ResourceRef> {
        vec![p::ResourceRef(format!("data-plan:{}", self.digest.0))]
    }
    fn scope(&self) -> p::Scope {
        match &self.operation {
            DataPlannedOperation::Lifecycle { plan } => plan.scope.clone(),
            _ => p::Scope("data:restore".into()),
        }
    }
}
impl ProductionRuntime {
    pub fn read_protected_artifact(
        &self,
        owner: &p::VerifiedPrincipal,
        id: &p::ContentRef,
    ) -> p::Result<Vec<u8>> {
        let _guard = self.data_read_guard()?;
        self.require_brain_owner(owner)?;
        let record = self
            .lifecycle_service()?
            .protected_content(owner, id)?
            .ok_or_else(|| p::Error("protected artifact is absent or erased".into()))?;
        self.services()
            .store
            .content_protection()?
            .ok_or_else(|| p::Error("content protection unavailable".into()))?
            .content_service()
            .decrypt(&record.envelope.binding(), &record.envelope)
    }
    fn data_directory(&self) -> p::Result<PathBuf> {
        let base = self
            .config()
            .store_path
            .parent()
            .unwrap_or_else(|| Path::new("."));
        let root = base.join("data-operations");
        std::fs::create_dir_all(&root)
            .map_err(|_| p::Error("data operations directory unavailable".into()))?;
        if std::fs::symlink_metadata(&root)
            .map_err(|_| p::Error("data operations directory unavailable".into()))?
            .file_type()
            .is_symlink()
        {
            return Err(p::Error(
                "data operations directory cannot be a symbolic link".into(),
            ));
        }
        root.canonicalize()
            .map_err(|_| p::Error("data operations directory unavailable".into()))
    }
    fn data_path(&self, name: &str) -> p::Result<PathBuf> {
        if name.is_empty()
            || name.len() > 96
            || name == "."
            || name == ".."
            || !name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
        {
            return Err(p::Error("data object must be an opaque local name".into()));
        }
        let path = self.data_directory()?.join(name);
        if let Ok(meta) = std::fs::symlink_metadata(&path) {
            if meta.file_type().is_symlink() || !meta.is_file() {
                return Err(p::Error("data object must be a regular local file".into()));
            }
        }
        Ok(path)
    }
    pub fn create_data_backup(
        &self,
        owner: &p::VerifiedPrincipal,
        target: &str,
    ) -> p::Result<forme_store::BackupManifest> {
        let _guard = self.data_read_guard()?;
        self.require_brain_owner(owner)?;
        self.services()
            .store
            .backup_service()
            .create_cold_backup(&self.data_path(target)?)
    }
    pub fn verify_data_backup(
        &self,
        owner: &p::VerifiedPrincipal,
        source: &str,
    ) -> p::Result<forme_store::BackupVerification> {
        self.require_brain_owner(owner)?;
        let verification = self
            .services()
            .store
            .backup_service()
            .verify_backup(&self.data_path(source)?)?;
        if verification.manifest.owner_digest.as_ref()
            != Some(&self.services().authority.owner_digest)
        {
            return Err(p::Error(
                "backup owner does not match the configured owner".into(),
            ));
        }
        Ok(verification)
    }
    pub fn prepare_data_restore(
        &self,
        owner: &p::VerifiedPrincipal,
        source: String,
        target: String,
    ) -> p::Result<DataOperationPlan> {
        self.require_brain_owner(owner)?;
        let verified = self.verify_data_backup(owner, &source)?;
        if self.data_path(&target)?.exists() {
            return Err(p::Error("restore target must be new".into()));
        }
        self.record_data_plan(
            owner,
            DataPlannedOperation::Restore {
                source,
                target,
                backup_digest: verified.manifest.database_digest,
            },
        )
    }
    pub fn prepare_data_lifecycle(
        &self,
        owner: &p::VerifiedPrincipal,
        scope: p::Scope,
        subject: p::SchemaDigest,
        operation: p::DataLifecycleOperation,
        expected_version: u64,
    ) -> p::Result<DataOperationPlan> {
        self.require_brain_owner(owner)?;
        let service = self.lifecycle_service()?;
        let request = forme_store::DataLifecycleRequest {
            schema_version: p::V1_CLOSURE_SCHEMA_VERSION,
            owner: owner.clone(),
            scope,
            subject_digest: subject,
            operation,
            expected_version,
            event_id: p::EventId("lifecycle:preview".into()),
            timestamp: now_ms(),
            remote_disposition: p::RemoteDeletionDisposition::Unknown,
            evidence: vec![],
        };
        self.record_data_plan(
            owner,
            DataPlannedOperation::Lifecycle {
                plan: service.plan(&request)?,
            },
        )
    }
    fn lifecycle_service(&self) -> p::Result<forme_store::DataLifecycleService> {
        let s = self.services().store.clone();
        let protection = s
            .content_protection()?
            .ok_or_else(|| p::Error("data protection is unavailable".into()))?;
        Ok(protection.lifecycle_service(s.as_ref().clone()))
    }
    fn record_data_plan(
        &self,
        owner: &p::VerifiedPrincipal,
        operation: DataPlannedOperation,
    ) -> p::Result<DataOperationPlan> {
        let now = now_ms();
        let nonce = p::Nonce(
            p::canonical_digest(&(
                owner,
                now,
                &operation,
                DATA_PLAN_NONCE.fetch_add(1, Ordering::Relaxed),
            ))?
            .0,
        );
        let mut plan = DataOperationPlan {
            schema_version: p::SchemaVersion(1),
            owner_digest: p::sha256_content_digest(owner.0.as_bytes()),
            operation,
            expires_at: now.saturating_add(300_000),
            nonce,
            digest: p::SchemaDigest(String::new()),
        };
        plan.refresh()?;
        self.data_event(
            &plan,
            "requested",
            p::EventPayload::ApprovalRequested(p::ApprovalRequestedPayload {
                approval_id: plan.approval(),
                action_summary: p::ActionSummary(
                    "Owner data operation requires explicit confirmation of this immutable plan"
                        .into(),
                ),
                risk: p::Risk::High,
                scope: plan.scope(),
                rollback_boundary: p::RollbackBoundary(
                    "data operation may be irreversible; restore never switches authority".into(),
                ),
                expires_at: plan.expires_at,
                choices: vec![
                    p::ApprovalChoice("approve-once".into()),
                    p::ApprovalChoice("deny".into()),
                ],
                requested_permissions: vec![p::PermissionRef("permission:owner-data".into())],
                affected_resources: plan.resources(),
                plan_digest: Some(p::PlanDigest(plan.digest.0.clone())),
                normalized_action_digest: Some(plan.digest.clone()),
                policy_version: Some(p::Version(1)),
                tool_schema_version: Some(p::Version(1)),
            }),
        )?;
        Ok(plan)
    }
    fn data_event(
        &self,
        plan: &DataOperationPlan,
        suffix: &str,
        payload: p::EventPayload,
    ) -> p::Result<()> {
        let id = p::EventId(format!("data-control:{}:{suffix}", plan.digest.0));
        for event in self.services().store.read_run(plan.run()) {
            let event = event?;
            if event.event_id == id {
                return if event.payload == payload {
                    Ok(())
                } else {
                    Err(p::Error("data operation audit identity conflict".into()))
                };
            }
        }
        self.services()
            .store
            .append(p::Event::new(
                id,
                plan.run(),
                None,
                payload,
                p::SchemaVersion(1),
                now_ms(),
                p::Provenance {
                    source: p::Source::OwnerControl,
                    actor: p::Actor::Owner,
                    trust_tier: p::TrustTier::OwnerInput,
                    caused_by: None,
                },
            ))
            .map(|_| ())
    }
    pub fn apply_data_operation(
        &self,
        owner: &p::VerifiedPrincipal,
        plan: DataOperationPlan,
        grant: DataOperationConfirmation,
    ) -> p::Result<DataOperationResult> {
        let services = self.services();
        let _exclusive=services.data_access.try_write().map_err(|_|p::Error("data operation requires the runtime to be idle; an execution or cognitive tick is active".into()))?;
        self.harness().require_data_quiescent()?;
        if !self
            .orchestration_active
            .lock()
            .map_err(|_| p::Error("orchestration registry unavailable".into()))?
            .is_empty()
        {
            return Err(p::Error(
                "data operation requires all routes to be idle".into(),
            ));
        }
        self.require_brain_owner(owner)?;
        plan.validate()?;
        let now = now_ms();
        if plan.owner_digest != p::sha256_content_digest(owner.0.as_bytes())
            || grant.schema_version.0 != 1
            || !grant.approved
            || grant.plan_digest != plan.digest
            || grant.nonce != plan.nonce
            || grant.use_by > plan.expires_at
            || now > grant.use_by
            || now > plan.expires_at
        {
            return Err(p::Error(
                "data approval is absent, expired or bound to a different plan".into(),
            ));
        }
        let recorded = self
            .services()
            .store
            .read_run(plan.run())
            .collect::<p::Result<Vec<_>>>()?;
        if !recorded.iter().any(|e|matches!(&e.payload,p::EventPayload::ApprovalRequested(r) if r.approval_id==plan.approval() && r.plan_digest.as_ref()==Some(&p::PlanDigest(plan.digest.0.clone())) && r.expires_at==plan.expires_at)) {return Err(p::Error("data plan has no persisted approval request".into()));}
        // Fresh validation happens before consuming the one-shot approval.
        if let DataPlannedOperation::Restore {
            source,
            target,
            backup_digest,
        } = &plan.operation
        {
            if self
                .verify_data_backup(owner, source)?
                .manifest
                .database_digest
                != *backup_digest
                || self.data_path(target)?.exists()
            {
                return Err(p::Error(
                    "restore source or target changed after approval".into(),
                ));
            }
        }
        if let DataPlannedOperation::Lifecycle { plan: cleanup } = &plan.operation {
            self.lifecycle_service()?
                .validate_approved_plan(&lifecycle_request(owner, cleanup, now), &cleanup.digest)?;
        }
        let claim = ApprovalConsumptionClaim {
            schema_version: forme_store::APPROVAL_CONSUMPTION_SCHEMA_VERSION,
            approval_id: plan.approval(),
            approver: owner.clone(),
            nonce: plan.nonce.clone(),
            plan_digest: p::PlanDigest(plan.digest.0.clone()),
            normalized_action_digest: plan.digest.clone(),
            affected_resources: plan.resources(),
            policy_version: p::Version(1),
            tool_schema_version: p::Version(1),
            now,
        };
        let fresh = self.services().store.claim_approval(&claim)?;
        if !fresh && !matches!(plan.operation, DataPlannedOperation::Lifecycle { .. }) {
            return Err(p::Error("data approval was already used; inspect the previous outcome instead of retrying restore".into()));
        }
        self.data_event(
            &plan,
            "approved",
            p::EventPayload::ApprovalResolved(p::ApprovalResolvedPayload {
                approval_id: plan.approval(),
                outcome: p::ApprovalOutcome::Granted,
                grant_ref: Some(p::ApprovalGrantRef(format!("data-grant:{}", plan.digest.0))),
            }),
        )?;
        let result = match &plan.operation {
            DataPlannedOperation::Restore { source, target, .. } => DataOperationResult::Restored(
                self.services()
                    .store
                    .backup_service()
                    .restore_to(&self.data_path(source)?, &self.data_path(target)?)?,
            ),
            DataPlannedOperation::Lifecycle { plan: cleanup } => {
                let r = lifecycle_request(owner, cleanup, now);
                let receipt = self.lifecycle_service()?.apply_plan(r, &cleanup.digest)?;
                self.services().memory.refresh_projection()?;
                self.services().cognition.refresh_projection()?;
                if cleanup.operation != p::DataLifecycleOperation::Retain {
                    self.harness().evict_data_caches()?;
                }
                DataOperationResult::Lifecycle(receipt)
            }
        };
        self.data_event(
            &plan,
            "complete",
            p::EventPayload::RunComplete(p::RunCompletePayload {
                stop_reason: p::StopReason("completed".into()),
                result_ref: None,
            }),
        )?;
        Ok(result)
    }
}
fn lifecycle_request(
    owner: &p::VerifiedPrincipal,
    cleanup: &forme_store::DataLifecyclePlan,
    now: p::Timestamp,
) -> forme_store::DataLifecycleRequest {
    forme_store::DataLifecycleRequest {
        schema_version: p::V1_CLOSURE_SCHEMA_VERSION,
        owner: owner.clone(),
        scope: cleanup.scope.clone(),
        subject_digest: cleanup.subject_digest.clone(),
        operation: cleanup.operation,
        expected_version: cleanup.expected_version,
        event_id: p::EventId(format!("data-lifecycle:{}", cleanup.digest.0)),
        timestamp: now,
        remote_disposition: p::RemoteDeletionDisposition::Unknown,
        evidence: vec![],
    }
}
