//! Installed data controls adapt the existing owner-bound data services.
use crate::{
    now_ms, DataOperationConfirmation, DataOperationPlan, DataOperationResult,
    DataPlannedOperation, ProductionRuntime,
};
use forme_protocol as p;
use forme_store::BackupManifest;
use serde::{Deserialize, Serialize};

mod journal;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    schema_version: p::SchemaVersion,
    request: p::DataWorkbenchCommand,
    receipt: p::DataCommandReceipt,
    plan: Option<DataOperationPlan>,
}

impl ProductionRuntime {
    pub fn query_data_workbench(
        &self,
        owner: &p::VerifiedPrincipal,
        query: p::DataWorkbenchQuery,
    ) -> p::Result<p::DataWorkbenchView> {
        self.require_brain_owner(owner)?;
        query.validate()?;
        match query.query {
            p::DataWorkbenchRead::Catalog { page } => self
                .services()
                .store
                .data_catalog(owner, &page)
                .map(p::DataWorkbenchView::Catalog),
            p::DataWorkbenchRead::Operations { page } => self
                .data_operation_list(owner, &page)
                .map(p::DataWorkbenchView::Operations),
            p::DataWorkbenchRead::Inspect { command } => {
                self.data_record(owner, &command).map(|record| {
                    p::DataWorkbenchView::Receipt(Box::new(p::DataReceiptView {
                        schema_version: p::SchemaVersion(1),
                        receipt: record.map(|r| r.receipt),
                    }))
                })
            }
            p::DataWorkbenchRead::VerifyBackup { source } => {
                let _guard = self.data_read_guard()?;
                let verification = self.verify_data_backup(owner, &source)?;
                Ok(p::DataWorkbenchView::Verification(backup_view(
                    &source,
                    &verification.manifest,
                    verification.is_valid(),
                )))
            }
        }
    }

    pub fn execute_data_workbench(
        &self,
        owner: &p::VerifiedPrincipal,
        request: p::DataWorkbenchCommand,
    ) -> p::Result<p::DataCommandReceipt> {
        self.require_brain_owner(owner)?;
        request.validate()?;
        // Serialise maintenance admission as well as a single command key.
        // Destructive services still acquire their own quiescent data lock.
        let lock = self
            .harness()
            .session_lock(&p::SessionId("maintenance:data-workbench".into()))?;
        let _guard = lock
            .lock()
            .map_err(|_| p::Error("data command queue unavailable".into()))?;
        if let Some(record) = self.data_record(owner, &request.command)? {
            if record.request != request {
                return Err(p::Error("data command identity conflict".into()));
            }
            return Ok(record.receipt);
        }
        let approved = match &request.action {
            p::DataWorkbenchAction::Apply {
                prepared_by,
                plan_digest,
            } => {
                let original = self
                    .data_record(owner, prepared_by)?
                    .ok_or_else(|| p::Error("original data plan is missing".into()))?;
                if original.receipt.state != p::DataCommandState::Prepared {
                    return Err(p::Error("original data plan is not confirmed".into()));
                }
                let plan = original
                    .plan
                    .ok_or_else(|| p::Error("original data plan is missing".into()))?;
                if plan.digest != *plan_digest || now_ms() > plan.expires_at {
                    return Err(p::Error("data plan changed or expired".into()));
                }
                // A new browser key must not create a second application of a
                // plan with an already-started or completed application.
                if self.data_plan_was_applied(owner, plan_digest)? {
                    return Err(p::Error(
                        "data plan already has an application; inspect its receipt".into(),
                    ));
                }
                Some(plan)
            }
            _ => {
                let _available = self.data_read_guard()?;
                None
            }
        };
        let receipt = p::DataCommandReceipt {
            schema_version: p::SchemaVersion(1),
            command: request.command.clone(),
            binding: p::canonical_digest(&(owner, &request))?,
            action: action_name(&request.action).into(),
            prepared_by: match &request.action {
                p::DataWorkbenchAction::Apply { prepared_by, .. } => Some(prepared_by.clone()),
                _ => None,
            },
            state: p::DataCommandState::Unknown,
            plan: None,
            backup: None,
            lifecycle: None,
        };
        let mut record = Record {
            schema_version: p::SchemaVersion(1),
            request,
            receipt,
            plan: None,
        };
        self.write_data_record(owner, "started", &record)?;
        // Any error after admission leaves the durable Unknown receipt. Never
        // infer rollback from an error, timeout, or an absent completion record.
        if self
            .perform_data_command(owner, &mut record, approved)
            .is_err()
        {
            return Ok(record.receipt);
        }
        self.write_data_record(owner, "confirmed", &record)?;
        Ok(record.receipt)
    }

    fn perform_data_command(
        &self,
        owner: &p::VerifiedPrincipal,
        record: &mut Record,
        approved: Option<DataOperationPlan>,
    ) -> p::Result<()> {
        match &record.request.action {
            p::DataWorkbenchAction::Backup { target } => {
                let manifest = self.create_data_backup(owner, target)?;
                record.receipt.backup = Some(backup_view(target, &manifest, false));
            }
            p::DataWorkbenchAction::PrepareRestore { source, target } => {
                record.plan =
                    Some(self.prepare_data_restore(owner, source.clone(), target.clone())?);
            }
            p::DataWorkbenchAction::PrepareLifecycle {
                scope,
                subject,
                operation,
                expected_version,
            } => {
                record.plan = Some(self.prepare_data_lifecycle(
                    owner,
                    scope.clone(),
                    subject.clone(),
                    *operation,
                    *expected_version,
                )?);
            }
            p::DataWorkbenchAction::Apply { .. } => {
                let plan = approved.ok_or_else(|| p::Error("data approval has no plan".into()))?;
                let confirmation = DataOperationConfirmation {
                    schema_version: p::SchemaVersion(1),
                    plan_digest: plan.digest.clone(),
                    nonce: plan.nonce.clone(),
                    use_by: plan.expires_at,
                    approved: true,
                };
                let target = if let DataPlannedOperation::Restore { target, .. } = &plan.operation {
                    Some(target.clone())
                } else {
                    None
                };
                match self.apply_data_operation(owner, plan, confirmation)? {
                    DataOperationResult::Restored(report) => {
                        let mut view = backup_view(
                            target.as_deref().unwrap_or(""),
                            &report.manifest,
                            report.verification.is_valid(),
                        );
                        if let (Some(owner), Some(store), Some(fence)) = (
                            report.manifest.owner_digest.as_ref(),
                            report.store_identity.as_ref(),
                            report.execution_fence_digest.as_ref(),
                        ) {
                            if view.verified {
                                let candidate = p::DataRestoreCandidateView {
                                    schema_version: p::SchemaVersion(1),
                                    candidate_ref: view.object.clone(),
                                    candidate_digest: p::canonical_digest(&report.manifest)?,
                                    owner_digest: owner.clone(),
                                    store_identity: store.clone(),
                                    execution_fence_digest: fence.clone(),
                                };
                                candidate.validate()?;
                                view.restoration = Some(candidate);
                            }
                        }
                        record.receipt.backup = Some(view);
                    }
                    DataOperationResult::Lifecycle(receipt) => {
                        record.receipt.lifecycle = Some(p::DataLifecycleView {
                            schema_version: p::SchemaVersion(1),
                            scope: receipt.scope,
                            subject: receipt.subject_digest,
                            operation: receipt.operation,
                            cleaned_projections: receipt.cleaned_projections.len(),
                            destroyed_keys: receipt.destroyed_key_digests.len(),
                            remote: receipt.remote_disposition,
                        });
                    }
                }
            }
        }
        record.receipt.plan = record.plan.as_ref().map(preview);
        record.receipt.state = if record.plan.is_some() {
            p::DataCommandState::Prepared
        } else {
            p::DataCommandState::Confirmed
        };
        Ok(())
    }

    fn data_operation_list(
        &self,
        owner: &p::VerifiedPrincipal,
        page: &p::DataPageQuery,
    ) -> p::Result<p::DataOperationList> {
        let mut operations = self
            .data_started_records(owner)?
            .into_iter()
            .map(|record| {
                self.data_record(owner, &record.request.command)
                    .map(|r| r.unwrap_or(record).receipt)
            })
            .collect::<p::Result<Vec<_>>>()?;
        operations.sort_by(|a, b| a.command.0.cmp(&b.command.0));
        let digest = p::canonical_digest(&operations)?;
        if page
            .snapshot
            .as_ref()
            .is_some_and(|expected| expected != &digest)
        {
            return Err(p::Error("data snapshot changed; restart pagination".into()));
        }
        let total = operations.len();
        let end = page.offset.saturating_add(page.limit).min(total);
        Ok(p::DataOperationList {
            schema_version: p::SchemaVersion(1),
            digest,
            total,
            next_offset: (end < total).then_some(end),
            operations: operations
                .into_iter()
                .skip(page.offset)
                .take(page.limit)
                .collect(),
        })
    }

    fn data_plan_was_applied(
        &self,
        owner: &p::VerifiedPrincipal,
        digest: &p::SchemaDigest,
    ) -> p::Result<bool> {
        Ok(self.data_started_records(owner)?.iter().any(|record|matches!(&record.request.action,p::DataWorkbenchAction::Apply {plan_digest,..} if plan_digest==digest)))
    }
}

fn action_name(action: &p::DataWorkbenchAction) -> &'static str {
    match action {
        p::DataWorkbenchAction::Backup { .. } => "Backup",
        p::DataWorkbenchAction::PrepareRestore { .. } => "PrepareRestore",
        p::DataWorkbenchAction::PrepareLifecycle { .. } => "PrepareLifecycle",
        p::DataWorkbenchAction::Apply { .. } => "Apply",
    }
}
fn backup_view(name: &str, manifest: &BackupManifest, verified: bool) -> p::DataBackupView {
    p::DataBackupView {
        schema_version: p::SchemaVersion(1),
        object: name.into(),
        digest: manifest.database_digest.clone(),
        events: manifest.event_count,
        created_at: manifest.created_at,
        verified,
        restoration: None,
    }
}
fn preview(plan: &DataOperationPlan) -> p::DataPlanPreview {
    let mut view = p::DataPlanPreview {
        schema_version: p::SchemaVersion(1),
        digest: plan.digest.clone(),
        expires_at: plan.expires_at,
        operation: String::new(),
        scope: None,
        subject: None,
        objects: 0,
        source: None,
        target: None,
        irreversible: false,
    };
    match &plan.operation {
        DataPlannedOperation::Restore { source, target, .. } => {
            view.operation = "RestoreToNewObject".into();
            view.source = Some(source.clone());
            view.target = Some(target.clone());
        }
        DataPlannedOperation::Lifecycle { plan } => {
            view.operation = format!("{:?}", plan.operation);
            view.scope = Some(plan.scope.clone());
            view.subject = Some(plan.subject_digest.clone());
            view.objects = plan
                .objects
                .iter()
                .map(|object| &object.object)
                .collect::<std::collections::BTreeSet<_>>()
                .len();
            view.irreversible = plan.operation != p::DataLifecycleOperation::Retain;
        }
    }
    view
}
