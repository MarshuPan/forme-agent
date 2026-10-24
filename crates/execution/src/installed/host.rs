use std::path::Path;
use std::sync::{Arc, Mutex};

use forme_protocol as p;

#[path = "host_configuration.rs"]
mod host_configuration;
#[path = "host_maintenance.rs"]
mod host_maintenance;
#[path = "host_network.rs"]
mod host_network;
pub use host_network::PreparedOwnerNetwork;
#[path = "host_native.rs"]
mod host_native;
#[path = "host_operations.rs"]
mod host_operations;
#[path = "host_restore.rs"]
mod host_restore;
#[cfg(test)]
#[path = "host_restore_tests.rs"]
mod host_restore_tests;

use super::{
    NativeServiceManager, ProductConfigStore, ProductJournal, ReleaseStager, SystemServiceManager,
};

/// Credentials for the two product control planes. The maintenance and
/// execution values are intentionally separate so one leaked domain cannot be
/// upgraded into the other by changing a request body.
#[derive(Clone)]
pub struct HostCredentials {
    maintenance: String,
    agent_execution: Option<String>,
    web_configuration: Option<String>,
}

impl HostCredentials {
    pub fn new(maintenance: String, agent_execution: Option<String>) -> p::Result<Self> {
        let credentials = Self {
            maintenance,
            agent_execution,
            web_configuration: None,
        };
        credentials.validate()?;
        Ok(credentials)
    }

    pub fn legacy_maintenance(maintenance: String) -> Self {
        Self {
            maintenance,
            agent_execution: None,
            web_configuration: None,
        }
    }

    pub fn maintenance(&self) -> &str {
        &self.maintenance
    }

    pub fn agent_execution(&self) -> Option<&str> {
        self.agent_execution.as_deref()
    }

    pub fn with_web_configuration(mut self, credential: Option<String>) -> p::Result<Self> {
        if let Some(value) = credential.as_deref() {
            validate_bearer(value, "Web configuration")?;
            if value == self.maintenance || self.agent_execution.as_deref() == Some(value) {
                return Err(p::Error("host credentials must be distinct".into()));
            }
        }
        self.web_configuration = credential;
        Ok(self)
    }

    pub fn web_configuration(&self) -> Option<&str> {
        self.web_configuration.as_deref()
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
        if let Some(configuration) = self.web_configuration.as_deref() {
            validate_bearer(configuration, "Web configuration")?;
            if configuration == self.maintenance
                || self.agent_execution.as_deref() == Some(configuration)
            {
                return Err(p::Error("host credentials must be distinct".into()));
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

    pub(crate) fn proof_ref(&self) -> &p::SchemaDigest {
        &self.proof_ref
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
    web_configuration_bearer: Option<String>,
    web_configuration_digest: Option<p::SchemaDigest>,
    product_config: Option<Arc<Mutex<ProductConfigStore>>>,
    release_stager: Option<Arc<ReleaseStager>>,
    restore_worker: Option<Arc<dyn super::RestoreWorkerProvider>>,
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
        Self::open_with_service_manager_and_config(
            journal_path,
            install_id,
            credentials,
            target,
            active_release,
            None,
            services,
        )
    }

    pub fn open_with_service_manager_and_config(
        journal_path: &Path,
        install_id: &str,
        credentials: HostCredentials,
        target: p::ProductTarget,
        active_release: String,
        product_config: Option<Arc<Mutex<ProductConfigStore>>>,
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
        let web_configuration_digest = credentials
            .web_configuration()
            .map(|value| p::canonical_digest(&value))
            .transpose()?;
        Ok(Self {
            journal: ProductJournal::open(journal_path, install_id)?,
            maintenance_bearer: credentials.maintenance().to_owned(),
            maintenance_digest,
            agent_bearer: credentials.agent_execution().map(str::to_owned),
            agent_digest,
            web_configuration_bearer: credentials.web_configuration().map(str::to_owned),
            web_configuration_digest,
            product_config,
            release_stager: None,
            restore_worker: None,
            target,
            install_id: install_id.into(),
            active_release,
            services,
        })
    }

    pub fn target(&self) -> p::ProductTarget {
        self.target
    }

    pub fn with_restore_worker(
        mut self,
        worker: Arc<dyn super::RestoreWorkerProvider>,
    ) -> p::Result<Self> {
        if worker.installation().install_id != self.install_id
            || worker.installation().target != self.target
        {
            return Err(p::Error(
                "restore worker belongs to another installation".into(),
            ));
        }
        self.restore_worker = Some(worker);
        Ok(self)
    }

    /// Attach a preflighted administrator-owned staging layout; never enables
    /// upgrade/restore without their independent lifecycle execution workers.
    pub fn with_release_stager(mut self, stager: Arc<ReleaseStager>) -> p::Result<Self> {
        if stager.install_ref() != self.install_id || stager.target() != self.target {
            return Err(p::Error(
                "release worker belongs to another installation".into(),
            ));
        }
        self.release_stager = Some(stager);
        Ok(self)
    }

    pub fn record_installation_verification(
        &self,
        manifest_digest: p::SchemaDigest,
        release_digest: p::SchemaDigest,
    ) -> p::Result<u64> {
        if let Some(worker) = &self.restore_worker {
            if worker.installation().digest()? != manifest_digest {
                return Err(p::Error(
                    "restore worker has a different installation manifest".into(),
                ));
            }
        }
        if let Some(stager) = &self.release_stager {
            if stager.installation_digest()? != manifest_digest {
                return Err(p::Error(
                    "release worker has a different installation manifest".into(),
                ));
            }
        }
        self.journal
            .record_installation_verification(manifest_digest, release_digest)
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
        actor: p::MaintenanceActorClass,
        now: p::Timestamp,
        receipt: p::SchemaDigest,
    ) -> p::Result<p::MaintenanceOperation> {
        let current = self.operation_state(operation_id)?;
        self.journal
            .begin_stage(operation_id, current.revision, stage, actor, now)?;
        let current = self.operation_state(operation_id)?;
        self.journal
            .finish_stage(operation_id, current.revision, stage, vec![receipt], now)?;
        self.operation_state(operation_id)
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
        let configuration = self
            .product_config
            .as_ref()
            .map(|store| {
                let mut store = store
                    .lock()
                    .map_err(|_| p::Error("managed product configuration is unavailable".into()))?;
                store.snapshot()
            })
            .transpose()?;
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
        let mut allowed_operations = vec![
            "inspect".into(),
            "start_core".into(),
            "stop_core".into(),
            "restart_core".into(),
            "plan".into(),
            "execute".into(),
        ];
        if self.product_config.is_some() && self.network_change_available()? {
            allowed_operations.push("configure_lan".into());
        }
        if self.release_stager.is_some() {
            allowed_operations.extend(["stage_signed_release".into(), "uninstall_plan".into()]);
        }
        if self
            .restore_worker
            .as_ref()
            .is_some_and(|worker| worker.preflight().is_ok())
        {
            allowed_operations.push("commit_restore".into());
        }
        Ok(p::HostStatus {
            schema_version: p::SchemaVersion(1),
            install_id_safe_ref: self.install_id.clone(),
            product_state,
            core_health,
            active_release: self.active_release.clone(),
            active_config_revision: configuration
                .as_ref()
                .map_or(0, |view| view.active_revision),
            acknowledged_config_revision: configuration
                .as_ref()
                .and_then(|view| view.acknowledged_revision),
            issue_refs: Vec::new(),
            allowed_operations,
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
        self.plan_authorized(auth, request, now)
    }

    fn execute(
        &self,
        auth: &HostAuth,
        operation_id: &str,
        now: p::Timestamp,
    ) -> p::Result<p::MaintenanceOperation> {
        self.verify_maintenance_auth(auth)?;
        self.execute_authorized(auth, operation_id, now)
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
