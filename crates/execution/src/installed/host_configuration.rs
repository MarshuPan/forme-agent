#![forbid(unsafe_code)]

use std::time::{SystemTime, UNIX_EPOCH};

use forme_protocol as p;

use super::{
    HostAuth, HostCredentialDomain, NativeServiceManager, ProductHostBroker,
    ISSUE_CORE_RECONCILIATION_UNAVAILABLE,
};

impl<M: NativeServiceManager> ProductHostBroker<M> {
    pub(super) fn verify_web_configuration_auth(&self, auth: &HostAuth) -> p::Result<()> {
        if auth.domain != HostCredentialDomain::OwnerMaintenance
            || auth.actor != p::MaintenanceActorClass::OwnerReauthenticated
            || self.web_configuration_digest.as_ref() != Some(&auth.proof_ref)
        {
            return Err(p::Error(
                "host configuration authorization is invalid".into(),
            ));
        }
        Ok(())
    }

    pub fn configuration(&self, auth: &HostAuth) -> p::Result<p::ProductConfigView> {
        self.configuration_with_schema(auth, p::PRODUCT_CONFIG_SCHEMA_VERSION)
    }

    pub fn configuration_with_schema(
        &self,
        auth: &HostAuth,
        version: p::SchemaVersion,
    ) -> p::Result<p::ProductConfigView> {
        self.verify_web_configuration_auth(auth)?;
        p::validate_product_settings_schema(version)?;
        let store = self
            .product_config
            .as_ref()
            .ok_or_else(|| p::Error("managed product configuration is unavailable".into()))?;
        let mut store = store
            .lock()
            .map_err(|_| p::Error("managed product configuration is unavailable".into()))?;
        store.snapshot_with_schema(version)
    }

    pub fn configuration_v2(&self, auth: &HostAuth) -> p::Result<p::ProductConfigViewV2> {
        self.verify_web_configuration_auth(auth)?;
        let store = self
            .product_config
            .as_ref()
            .ok_or_else(|| p::Error("managed product configuration is unavailable".into()))?;
        store
            .lock()
            .map_err(|_| p::Error("managed product configuration is unavailable".into()))?
            .snapshot_settings_v2()
    }

    pub fn apply_configuration(
        &self,
        auth: &HostAuth,
        patch: p::ProductConfigPatch,
    ) -> p::Result<p::ProductConfigResponse> {
        self.verify_web_configuration_auth(auth)?;
        let store = self
            .product_config
            .as_ref()
            .ok_or_else(|| p::Error("managed product configuration is unavailable".into()))?;
        let response = {
            let mut store = store
                .lock()
                .map_err(|_| p::Error("managed product configuration is unavailable".into()))?;
            if self.journal.operations(0)?.iter().any(|operation| {
                !operation.stage.is_terminal()
                    && self
                        .journal
                        .command_kind(&operation.operation_id)
                        .map_or(true, |kind| kind.is_lifecycle_transaction())
            }) {
                return Err(p::Error(
                    "configuration cannot change during an unresolved lifecycle operation".into(),
                ));
            }
            store.apply(patch)?
        };
        let mut response = response;
        match self.reconcile_core_after_configuration(
            auth,
            response.view.owner.as_ref(),
            response.view.active_revision,
            response.view.configured,
            response.view.pending_restart,
        ) {
            Ok(activation) => response.core_activation = activation,
            Err(_) => response
                .view
                .issue_refs
                .push(ISSUE_CORE_RECONCILIATION_UNAVAILABLE.into()),
        }
        Ok(response)
    }

    pub fn apply_configuration_v2(
        &self,
        auth: &HostAuth,
        patch: p::ProductConfigPatchV2,
    ) -> p::Result<p::ProductConfigResponseV2> {
        self.verify_web_configuration_auth(auth)?;
        let store = self
            .product_config
            .as_ref()
            .ok_or_else(|| p::Error("managed product configuration is unavailable".into()))?;
        let response = {
            let mut store = store
                .lock()
                .map_err(|_| p::Error("managed product configuration is unavailable".into()))?;
            if self.journal.operations(0)?.iter().any(|operation| {
                !operation.stage.is_terminal()
                    && self
                        .journal
                        .command_kind(&operation.operation_id)
                        .map_or(true, |kind| kind.is_lifecycle_transaction())
            }) {
                return Err(p::Error(
                    "configuration cannot change during an unresolved lifecycle operation".into(),
                ));
            }
            store.apply_settings_v2(patch)?
        };
        let mut response = response;
        match self.reconcile_core_after_configuration_v2(
            auth,
            response.view.owner.as_ref(),
            response.view.active_revision,
            response.view.configured,
            response.view.pending_restart,
        ) {
            Ok(activation) => response.core_activation = activation,
            Err(_) => response
                .view
                .issue_refs
                .push(ISSUE_CORE_RECONCILIATION_UNAVAILABLE.into()),
        }
        Ok(response)
    }

    pub fn bind_configuration_owner(
        &self,
        auth: &HostAuth,
        binding: p::ProductConfigOwnerBinding,
    ) -> p::Result<p::ProductConfigResponse> {
        self.verify_web_configuration_auth(auth)?;
        let store = self
            .product_config
            .as_ref()
            .ok_or_else(|| p::Error("managed product configuration is unavailable".into()))?;
        store
            .lock()
            .map_err(|_| p::Error("managed product configuration is unavailable".into()))?
            .bind_owner(binding)
    }

    pub fn validate_configuration(
        &self,
        auth: &HostAuth,
        request: p::ProductConfigValidationRequest,
    ) -> p::Result<p::ProductConfigValidationReport> {
        self.verify_web_configuration_auth(auth)?;
        if request.schema_version != p::PRODUCT_CONFIG_SCHEMA_VERSION {
            return Err(p::Error(
                "unsupported configuration validation schema".into(),
            ));
        }
        let store = self
            .product_config
            .as_ref()
            .ok_or_else(configuration_unavailable)?;
        store
            .lock()
            .map_err(|_| configuration_unavailable())?
            .validate_revision(request.revision, &request.owner)
    }

    /// Only the Web service's operation-authenticated maintenance adapter may
    /// enter here. The configuration lock spans CAS validation and native IO.
    /// An existing operation, including an unresolved one, is never re-executed.
    pub fn activate_configuration(
        &self,
        auth: &HostAuth,
        request: p::ProductCoreActivation,
        now: p::Timestamp,
    ) -> p::Result<p::ProductCoreActivationResponse> {
        self.verify_web_configuration_auth(auth)?;
        let store = self
            .product_config
            .as_ref()
            .ok_or_else(configuration_unavailable)?;
        let mut store = store.lock().map_err(|_| configuration_unavailable())?;
        self.activate_configuration_locked(auth, request, now, &mut store)
    }

    pub(super) fn activate_configuration_locked(
        &self,
        auth: &HostAuth,
        request: p::ProductCoreActivation,
        now: p::Timestamp,
        store: &mut super::ProductConfigStore,
    ) -> p::Result<p::ProductCoreActivationResponse> {
        self.verify_web_configuration_auth(auth)?;
        request.validate()?;
        let kind = p::MaintenanceKind::ApplyValidatedConfig {
            config_revision: request.expected_revision,
            validation_digest: request.digest()?,
        };
        let current = store.snapshot()?;
        if current.owner.as_ref() != Some(&request.owner) {
            return Err(p::Error(
                "configuration owner does not match the installed owner".into(),
            ));
        }
        let operations = self.journal.operations(0)?;
        if let Some(existing) = operations
            .iter()
            .find(|operation| operation.operation_id == request.operation_id)
        {
            if self.journal.command(&request.operation_id)?.kind != kind {
                return Err(p::Error(
                    "configuration operation id was reused with different content".into(),
                ));
            }
            return self.activation_response(existing.clone(), store);
        }
        if operations
            .iter()
            .any(|operation| !operation.stage.is_terminal() || operation.pending_stage.is_some())
        {
            return Err(p::Error(
                "maintenance recovery is required before another activation".into(),
            ));
        }
        if now >= request.requested_deadline
            || request.requested_deadline.saturating_sub(now) > 5 * 60 * 1000
        {
            return Err(p::Error("core activation authorization expired".into()));
        }
        store.validate_revision(request.expected_revision, &request.owner)?;
        let active_release = self.active_release_id()?;
        let operation = self.journal.plan(
            p::MaintenanceCommand {
                schema_version: p::SchemaVersion(1),
                operation_id: request.operation_id.clone(),
                idempotency_key: request.operation_id.clone(),
                expected_revision: self.journal.revision()?,
                actor_proof: p::MaintenanceProofRef {
                    schema_version: p::SchemaVersion(1),
                    proof_ref: auth.proof_ref.0.clone(),
                },
                kind,
                requested_deadline: request.requested_deadline,
            },
            auth.actor,
            p::MaintenanceReferences {
                schema_version: p::SchemaVersion(1),
                original_release_ref: active_release.clone(),
                target_release_ref: active_release,
                data_candidate_ref: None,
                backup_ref: None,
            },
            now,
        )?;
        let operation = self.configuration_stage(
            &operation,
            p::MaintenanceStage::Validated,
            auth.actor,
            now,
            p::canonical_digest(&"configuration-static-validation-passed")?,
        )?;
        self.journal.begin_stage(
            &operation.operation_id,
            operation.revision,
            p::MaintenanceStage::Checking,
            auth.actor,
            now,
        )?;
        let outcome = self.apply_configuration_action(request.action);
        let operation = match outcome {
            Ok(receipt) => {
                let operation = self.journal.operation(&request.operation_id)?;
                self.journal.finish_stage(
                    &request.operation_id,
                    operation.revision,
                    p::MaintenanceStage::Checking,
                    vec![receipt],
                    now,
                )?;
                let operation = self.journal.operation(&request.operation_id)?;
                self.configuration_stage(
                    &operation,
                    p::MaintenanceStage::Committed,
                    auth.actor,
                    now,
                    p::canonical_digest(&"configuration-service-change-observed")?,
                )?
            }
            Err(_) => {
                self.journal.require_recovery(
                    &request.operation_id,
                    self.journal.revision()?,
                    p::canonical_digest(&"configuration-service-outcome-unverified")?,
                    now,
                )?;
                self.journal.operation(&request.operation_id)?
            }
        };
        self.activation_response(operation, store)
    }

    pub fn configuration_operation(
        &self,
        auth: &HostAuth,
        id: &str,
    ) -> p::Result<p::MaintenanceOperation> {
        self.verify_web_configuration_auth(auth)?;
        p::validate_product_id(id)?;
        if !matches!(
            self.journal.command(id)?.kind,
            p::MaintenanceKind::ApplyValidatedConfig { .. }
        ) {
            return Err(p::Error(
                "operation is outside the Web configuration domain".into(),
            ));
        }
        self.journal.operation(id)
    }

    fn activation_response(
        &self,
        operation: p::MaintenanceOperation,
        store: &mut super::ProductConfigStore,
    ) -> p::Result<p::ProductCoreActivationResponse> {
        Ok(p::ProductCoreActivationResponse {
            schema_version: p::SchemaVersion(1),
            operation,
            configuration: store.snapshot()?,
            service: self.services.inspect(p::BuiltinServiceId::Brain).unwrap_or(
                p::NativeServiceStatus {
                    schema_version: p::SchemaVersion(1),
                    service: p::BuiltinServiceId::Brain,
                    state: p::NativeServiceState::Unknown,
                },
            ),
        })
    }

    fn reconcile_core_after_configuration(
        &self,
        auth: &HostAuth,
        owner: Option<&p::OwnerPrincipalRef>,
        revision: u64,
        configured: bool,
        pending_restart: bool,
    ) -> p::Result<Option<p::ProductCoreActivationResponse>> {
        if !self.automatic_core_activation {
            return Ok(None);
        }
        let Some(owner) = owner else {
            return Ok(None);
        };
        if !configured || revision == 0 {
            return Ok(None);
        }
        let result =
            self.reconcile_core(auth, owner, revision, pending_restart, current_timestamp()?);
        self.mark_startup_reconciliation_failed(result.is_err());
        result
    }

    fn reconcile_core_after_configuration_v2(
        &self,
        auth: &HostAuth,
        owner: Option<&p::OwnerPrincipalRef>,
        revision: u64,
        configured: bool,
        pending_restart: bool,
    ) -> p::Result<Option<p::ProductCoreActivationResponseV2>> {
        let Some(activation) = self.reconcile_core_after_configuration(
            auth,
            owner,
            revision,
            configured,
            pending_restart,
        )?
        else {
            return Ok(None);
        };
        let store = self
            .product_config
            .as_ref()
            .ok_or_else(configuration_unavailable)?;
        let configuration = store
            .lock()
            .map_err(|_| configuration_unavailable())?
            .snapshot_settings_v2()?;
        Ok(Some(p::ProductCoreActivationResponseV2 {
            schema_version: p::SchemaVersion(1),
            operation: activation.operation,
            configuration,
            service: activation.service,
        }))
    }

    fn configuration_stage(
        &self,
        operation: &p::MaintenanceOperation,
        stage: p::MaintenanceStage,
        actor: p::MaintenanceActorClass,
        now: p::Timestamp,
        receipt: p::SchemaDigest,
    ) -> p::Result<p::MaintenanceOperation> {
        self.journal.begin_stage(
            &operation.operation_id,
            operation.revision,
            stage,
            actor,
            now,
        )?;
        let current = self.journal.operation(&operation.operation_id)?;
        self.journal.finish_stage(
            &operation.operation_id,
            current.revision,
            stage,
            vec![receipt],
            now,
        )?;
        self.journal.operation(&operation.operation_id)
    }

    fn apply_configuration_action(
        &self,
        action: p::ProductCoreAction,
    ) -> p::Result<p::SchemaDigest> {
        let actions: &[p::NativeServiceAction] = match action {
            p::ProductCoreAction::Start => &[p::NativeServiceAction::Start],
            p::ProductCoreAction::Restart => {
                &[p::NativeServiceAction::Stop, p::NativeServiceAction::Start]
            }
        };
        let mut receipts = Vec::new();
        for action in actions {
            let receipt = self.services.apply(p::ValidatedServiceChange {
                schema_version: p::SchemaVersion(1),
                service: p::BuiltinServiceId::Brain,
                action: *action,
            })?;
            let expected = match action {
                p::NativeServiceAction::Start => p::NativeServiceState::Running,
                p::NativeServiceAction::Stop => p::NativeServiceState::Stopped,
            };
            if receipt.schema_version != p::SchemaVersion(1)
                || receipt.service != p::BuiltinServiceId::Brain
                || receipt.requested != *action
                || receipt.observed != expected
            {
                return Err(p::Error(
                    "native service change has no verified receipt".into(),
                ));
            }
            receipts.push(receipt);
        }
        p::canonical_digest(&receipts)
    }

    pub fn authenticate_web_configuration(&self, bearer: &str) -> p::Result<HostAuth> {
        let Some(expected_bearer) = self.web_configuration_bearer.as_deref() else {
            return Err(p::Error(
                "Web configuration authentication is not configured".into(),
            ));
        };
        let expected = expected_bearer.as_bytes();
        let actual = bearer.as_bytes();
        if expected.len() != actual.len()
            || expected
                .iter()
                .zip(actual)
                .fold(0_u8, |difference, (left, right)| {
                    difference | (left ^ right)
                })
                != 0
        {
            return Err(p::Error(
                "Web configuration authorization is invalid".into(),
            ));
        }
        Ok(HostAuth {
            proof_ref: self
                .web_configuration_digest
                .clone()
                .expect("validated Web configuration digest"),
            actor: p::MaintenanceActorClass::OwnerReauthenticated,
            domain: HostCredentialDomain::OwnerMaintenance,
        })
    }
}

pub(super) fn configuration_unavailable() -> p::Error {
    p::Error("managed product configuration is unavailable".into())
}

pub(super) fn current_timestamp() -> p::Result<p::Timestamp> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_millis()).ok())
        .ok_or_else(|| p::Error("system clock is unavailable".into()))
}
