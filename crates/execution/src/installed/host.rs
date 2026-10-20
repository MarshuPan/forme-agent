use std::path::Path;

use forme_protocol as p;

use super::{NativeServiceManager, ProductJournal, SystemServiceManager};

/// Credentials for the two product control planes. The maintenance and
/// execution values are intentionally separate so one leaked domain cannot be
/// upgraded into the other by changing a request body.
#[derive(Clone)]
pub struct HostCredentials {
    maintenance: String,
    agent_execution: Option<String>,
}

impl HostCredentials {
    pub fn new(maintenance: String, agent_execution: Option<String>) -> p::Result<Self> {
        let credentials = Self {
            maintenance,
            agent_execution,
        };
        credentials.validate()?;
        Ok(credentials)
    }

    pub fn legacy_maintenance(maintenance: String) -> Self {
        Self {
            maintenance,
            agent_execution: None,
        }
    }

    pub fn maintenance(&self) -> &str {
        &self.maintenance
    }

    pub fn agent_execution(&self) -> Option<&str> {
        self.agent_execution.as_deref()
    }

    fn validate(&self) -> p::Result<()> {
        validate_bearer(&self.maintenance, "maintenance")?;
        if let Some(agent) = self.agent_execution.as_deref() {
            validate_bearer(agent, "agent execution")?;
            if agent == self.maintenance {
                return Err(p::Error(
                    "maintenance and agent credentials must be distinct".into(),
                ));
            }
        }
        Ok(())
    }
}

fn validate_bearer(value: &str, domain: &str) -> p::Result<()> {
    if value.len() < 32 || value.chars().any(char::is_whitespace) {
        return Err(p::Error(format!("{domain} credential is malformed")));
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostCredentialDomain {
    OwnerMaintenance,
    AgentExecution,
}

/// The only authenticated capability accepted by the local maintenance broker.
/// The bearer value is kept by the caller; the broker exposes only its digest.
#[derive(Clone)]
pub struct HostAuth {
    proof_ref: p::SchemaDigest,
    actor: p::MaintenanceActorClass,
    domain: HostCredentialDomain,
}

impl HostAuth {
    pub fn actor(&self) -> p::MaintenanceActorClass {
        self.actor
    }

    pub fn domain(&self) -> HostCredentialDomain {
        self.domain
    }
}

pub trait HostManagement: Send + Sync {
    fn authenticate(&self, bearer: &str) -> p::Result<HostAuth>;
    fn status(&self, auth: &HostAuth) -> p::Result<p::HostStatus>;
    fn plan(
        &self,
        auth: &HostAuth,
        request: p::HostManagementRequest,
        now: p::Timestamp,
    ) -> p::Result<p::MaintenanceOperation>;
    fn execute(
        &self,
        auth: &HostAuth,
        operation_id: &str,
        now: p::Timestamp,
    ) -> p::Result<p::MaintenanceOperation>;
    fn operation(&self, auth: &HostAuth, operation_id: &str) -> p::Result<p::MaintenanceOperation>;
}

pub struct ProductHostBroker<M = SystemServiceManager> {
    journal: ProductJournal,
    maintenance_bearer: String,
    maintenance_digest: p::SchemaDigest,
    agent_bearer: Option<String>,
    agent_digest: Option<p::SchemaDigest>,
    target: p::ProductTarget,
    install_id: String,
    active_release: String,
    services: M,
}

impl ProductHostBroker<SystemServiceManager> {
    pub fn open(
        journal_path: &Path,
        install_id: &str,
        bearer: String,
        target: p::ProductTarget,
        active_release: String,
    ) -> p::Result<Self> {
        Self::open_with_service_manager(
            journal_path,
            install_id,
            HostCredentials::legacy_maintenance(bearer),
            target,
            active_release,
            SystemServiceManager,
        )
    }
}

impl<M: NativeServiceManager> ProductHostBroker<M> {
    pub fn open_with_service_manager(
        journal_path: &Path,
        install_id: &str,
        credentials: HostCredentials,
        target: p::ProductTarget,
        active_release: String,
        services: M,
    ) -> p::Result<Self> {
        credentials.validate()?;
        p::validate_product_id(install_id)?;
        p::validate_release_id(&active_release)?;
        let maintenance_digest = p::canonical_digest(&credentials.maintenance())?;
        let agent_digest = credentials
            .agent_execution()
            .map(|value| p::canonical_digest(&value))
            .transpose()?;
        Ok(Self {
            journal: ProductJournal::open(journal_path, install_id)?,
            maintenance_bearer: credentials.maintenance().to_owned(),
            maintenance_digest,
            agent_bearer: credentials.agent_execution().map(str::to_owned),
            agent_digest,
            target,
            install_id: install_id.into(),
            active_release,
            services,
        })
    }

    pub fn target(&self) -> p::ProductTarget {
        self.target
    }

    fn verify_maintenance_auth(&self, auth: &HostAuth) -> p::Result<()> {
        if auth.domain != HostCredentialDomain::OwnerMaintenance
            || auth.actor != p::MaintenanceActorClass::LocalAdministrator
            || auth.proof_ref != self.maintenance_digest
        {
            return Err(p::Error("host management authorization is invalid".into()));
        }
        Ok(())
    }

    fn operation_state(&self, operation_id: &str) -> p::Result<p::MaintenanceOperation> {
        self.journal.operation(operation_id)
    }

    fn record_stage(
        &self,
        operation_id: &str,
        stage: p::MaintenanceStage,
        now: p::Timestamp,
        receipt: p::SchemaDigest,
    ) -> p::Result<p::MaintenanceOperation> {
        let current = self.operation_state(operation_id)?;
        self.journal.begin_stage(
            operation_id,
            current.revision,
            stage,
            p::MaintenanceActorClass::LocalAdministrator,
            now,
        )?;
        let current = self.operation_state(operation_id)?;
        self.journal
            .finish_stage(operation_id, current.revision, stage, vec![receipt], now)?;
        self.operation_state(operation_id)
    }

    fn apply_native(&self, kind: &p::MaintenanceKind) -> p::Result<p::SchemaDigest> {
        let receipt = match kind {
            p::MaintenanceKind::StartCore => self.services.apply(p::ValidatedServiceChange {
                schema_version: p::SchemaVersion(1),
                service: p::BuiltinServiceId::Brain,
                action: p::NativeServiceAction::Start,
            })?,
            p::MaintenanceKind::StopCore => self.services.apply(p::ValidatedServiceChange {
                schema_version: p::SchemaVersion(1),
                service: p::BuiltinServiceId::Brain,
                action: p::NativeServiceAction::Stop,
            })?,
            p::MaintenanceKind::RestartCore => {
                self.services.apply(p::ValidatedServiceChange {
                    schema_version: p::SchemaVersion(1),
                    service: p::BuiltinServiceId::Brain,
                    action: p::NativeServiceAction::Stop,
                })?;
                self.services.apply(p::ValidatedServiceChange {
                    schema_version: p::SchemaVersion(1),
                    service: p::BuiltinServiceId::Brain,
                    action: p::NativeServiceAction::Start,
                })?
            }
            p::MaintenanceKind::Inspect => {
                return p::canonical_digest(&self.services.inspect(p::BuiltinServiceId::Brain)?);
            }
            _ => {
                return Err(p::Error(
                    "maintenance operation requires its typed lifecycle worker".into(),
                ));
            }
        };
        let expected = match receipt.requested {
            p::NativeServiceAction::Start => p::NativeServiceState::Running,
            p::NativeServiceAction::Stop => p::NativeServiceState::Stopped,
        };
        if receipt.observed != expected {
            return Err(p::Error(
                "native service has no settled result; probe the original operation".into(),
            ));
        }
        p::canonical_digest(&receipt)
    }

    fn status_for(&self) -> p::Result<p::HostStatus> {
        let operations = self.journal.operations(0)?;
        let recovery = operations.iter().any(|operation| {
            operation.stage == p::MaintenanceStage::RecoveryRequired
                || operation.pending_stage.is_some()
        });
        let pending = operations.iter().any(|operation| {
            operation.stage != p::MaintenanceStage::Committed || operation.pending_stage.is_some()
        });
        let native = self.services.inspect(p::BuiltinServiceId::Brain)?;
        let core_health = match native.state {
            // A running OS process has not yet proved owner/store/composition/config readiness.
            p::NativeServiceState::Running => p::CoreHealth::Starting,
            p::NativeServiceState::Stopped => p::CoreHealth::Stopped,
            p::NativeServiceState::Starting => p::CoreHealth::Starting,
            p::NativeServiceState::Stopping => p::CoreHealth::Degraded,
            p::NativeServiceState::NotInstalled => p::CoreHealth::Unconfigured,
            p::NativeServiceState::Failed | p::NativeServiceState::Unknown => {
                p::CoreHealth::Unreachable
            }
        };
        let product_state = if recovery {
            p::ProductState::RecoveryRequired
        } else if pending {
            p::ProductState::Maintenance
        } else {
            match core_health {
                p::CoreHealth::Starting => p::ProductState::Starting,
                p::CoreHealth::Stopped => p::ProductState::Stopped,
                p::CoreHealth::Unconfigured => p::ProductState::ConfigurationRequired,
                _ => p::ProductState::Degraded,
            }
        };
        Ok(p::HostStatus {
            schema_version: p::SchemaVersion(1),
            install_id_safe_ref: self.install_id.clone(),
            product_state,
            core_health,
            active_release: self.active_release.clone(),
            active_config_revision: 0,
            acknowledged_config_revision: None,
            issue_refs: Vec::new(),
            allowed_operations: vec![
                "inspect".into(),
                "start_core".into(),
                "stop_core".into(),
                "restart_core".into(),
                "plan".into(),
                "execute".into(),
            ],
        })
    }
}

impl<M: NativeServiceManager> HostManagement for ProductHostBroker<M> {
    fn authenticate(&self, bearer: &str) -> p::Result<HostAuth> {
        let expected = self.maintenance_bearer.as_bytes();
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
            return Err(p::Error("host management bearer is invalid".into()));
        }
        Ok(HostAuth {
            proof_ref: self.maintenance_digest.clone(),
            actor: p::MaintenanceActorClass::LocalAdministrator,
            domain: HostCredentialDomain::OwnerMaintenance,
        })
    }

    fn status(&self, auth: &HostAuth) -> p::Result<p::HostStatus> {
        self.verify_maintenance_auth(auth)?;
        self.status_for()
    }

    fn plan(
        &self,
        auth: &HostAuth,
        request: p::HostManagementRequest,
        now: p::Timestamp,
    ) -> p::Result<p::MaintenanceOperation> {
        self.verify_maintenance_auth(auth)?;
        request.validate()?;
        if request.command.actor_proof.proof_ref != auth.proof_ref.0 {
            return Err(p::Error(
                "maintenance proof is not bound to this request".into(),
            ));
        }
        if request.references.original_release_ref != self.active_release {
            return Err(p::Error(
                "maintenance command is bound to a stale active release".into(),
            ));
        }
        if !matches!(
            &request.command.kind,
            p::MaintenanceKind::Inspect
                | p::MaintenanceKind::StartCore
                | p::MaintenanceKind::StopCore
                | p::MaintenanceKind::RestartCore
        ) {
            return Err(p::Error(
                "maintenance operation has no installed lifecycle worker".into(),
            ));
        }
        self.journal
            .plan(request.command, auth.actor, request.references, now)
    }

    fn execute(
        &self,
        auth: &HostAuth,
        operation_id: &str,
        now: p::Timestamp,
    ) -> p::Result<p::MaintenanceOperation> {
        self.verify_maintenance_auth(auth)?;
        p::validate_product_id(operation_id)?;
        let operation = self.operation_state(operation_id)?;
        if operation.stage == p::MaintenanceStage::Committed {
            return Ok(operation);
        }
        if operation.stage == p::MaintenanceStage::RecoveryRequired {
            return Err(p::Error(
                "operation requires explicit recovery review".into(),
            ));
        }
        if operation.pending_stage.is_some() {
            return Err(p::Error("operation has an unresolved stage intent".into()));
        }
        let command = self.journal.command(operation_id)?;
        if now >= command.requested_deadline
            || operation.original_release_ref != self.active_release
        {
            return Err(p::Error(
                "maintenance plan expired or its active release changed".into(),
            ));
        }
        if operation.stage != p::MaintenanceStage::Planned {
            return Err(p::Error(
                "interrupted maintenance requires read-only recovery review".into(),
            ));
        }
        let operation = self.record_stage(
            operation_id,
            p::MaintenanceStage::Validated,
            now,
            p::canonical_digest(&"host-validation-passed")?,
        )?;
        if operation.command_digest.0.is_empty() {
            return Err(p::Error("maintenance command digest is unavailable".into()));
        }
        // Persist the native action's intent BEFORE invoking any service control.
        // The CAS admits only one caller; crashes leave this exact intent unresolved.
        self.journal.begin_stage(
            operation_id,
            operation.revision,
            p::MaintenanceStage::Checking,
            auth.actor,
            now,
        )?;
        let receipt = match self.apply_native(&command.kind) {
            Ok(receipt) => receipt,
            Err(error) => {
                let issue = p::canonical_digest(&error.to_string())?;
                self.journal.require_recovery(
                    operation_id,
                    self.journal.revision()?,
                    issue,
                    now,
                )?;
                return self.operation_state(operation_id);
            }
        };
        self.journal.finish_stage(
            operation_id,
            self.journal.revision()?,
            p::MaintenanceStage::Checking,
            vec![receipt],
            now,
        )?;
        let operation = self.operation_state(operation_id)?;
        let current = operation.revision;
        self.journal.begin_stage(
            operation_id,
            current,
            p::MaintenanceStage::Committed,
            auth.actor,
            now,
        )?;
        let current = self.operation_state(operation_id)?;
        self.journal.finish_stage(
            operation_id,
            current.revision,
            p::MaintenanceStage::Committed,
            vec![p::canonical_digest(&"host-commit-observed")?],
            now,
        )?;
        self.operation_state(operation_id)
    }

    fn operation(&self, auth: &HostAuth, operation_id: &str) -> p::Result<p::MaintenanceOperation> {
        self.verify_maintenance_auth(auth)?;
        p::validate_product_id(operation_id)?;
        self.operation_state(operation_id)
    }
}

impl<M: NativeServiceManager> ProductHostBroker<M> {
    /// Authenticate the execution domain without granting it any maintenance
    /// operation. The returned auth can only be consumed by a future typed
    /// worker dispatch surface.
    pub fn authenticate_agent(&self, bearer: &str) -> p::Result<HostAuth> {
        let Some(expected_bearer) = self.agent_bearer.as_deref() else {
            return Err(p::Error(
                "agent execution authentication is not configured".into(),
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
            return Err(p::Error("agent execution authorization is invalid".into()));
        }
        Ok(HostAuth {
            proof_ref: self.agent_digest.clone().expect("validated agent digest"),
            actor: p::MaintenanceActorClass::OwnerReauthenticated,
            domain: HostCredentialDomain::AgentExecution,
        })
    }
}
