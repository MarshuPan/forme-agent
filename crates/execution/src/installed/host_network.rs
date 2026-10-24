//! A single admitted network change, dispatched only after its response closes.
use forme_protocol as p;

use super::{HostAuth, NativeServiceManager, ProductHostBroker};

/// In-process, non-cloneable dispatch authority. A journal entry alone cannot
/// recreate it after restart, response loss or a repeated POST.
pub struct PreparedOwnerNetwork {
    pub(super) auth: HostAuth,
    pub(super) operation_id: String,
    pub(super) command_digest: p::SchemaDigest,
    pub(super) owner: p::OwnerPrincipalRef,
}

impl<M: NativeServiceManager> ProductHostBroker<M> {
    pub(super) fn network_change_available(&self) -> p::Result<bool> {
        let Some(config) = &self.product_config else {
            return Ok(false);
        };
        let mut config = config
            .lock()
            .map_err(|_| p::Error("managed product configuration is unavailable".into()))?;
        self.network_change_preflight(&mut config)
    }

    pub(super) fn network_change_preflight(
        &self,
        config: &mut super::ProductConfigStore,
    ) -> p::Result<bool> {
        let web = self.services.inspect(p::BuiltinServiceId::Web)?;
        if web.schema_version != p::SchemaVersion(1)
            || web.service != p::BuiltinServiceId::Web
            || !matches!(
                web.state,
                p::NativeServiceState::Running | p::NativeServiceState::Stopped
            )
        {
            return Ok(false);
        }
        if !config.network_is_managed() {
            return Ok(false);
        }
        let desired = config.snapshot()?.network;
        let previous = config.active_network()?;
        if desired.bind.ip().is_loopback() && previous.bind.ip().is_loopback() {
            return Ok(true);
        }
        Ok(self.services.supports_firewall_reconciliation()
            && self
                .services
                .validate_firewall_reconciliation(&previous, &desired)
                .is_ok())
    }

    /// The transport must close the successful response before consuming the
    /// ticket, allowing Web to release the request that its shutdown will drain.
    pub fn prepare_owner_network(
        &self,
        auth: &HostAuth,
        request: p::ProductMaintenanceRequest,
        now: p::Timestamp,
    ) -> p::Result<(p::MaintenanceOperation, Option<PreparedOwnerNetwork>)> {
        if !matches!(
            request.action,
            p::ProductMaintenanceAction::ConfigureLan { .. }
        ) {
            return Err(p::Error(
                "deferred owner operation must be a network change".into(),
            ));
        }
        self.owner_maintenance_inner(auth, request, now, true)
    }

    pub fn execute_owner_network(
        &self,
        prepared: PreparedOwnerNetwork,
        now: p::Timestamp,
    ) -> p::Result<p::MaintenanceOperation> {
        self.verify_web_configuration_auth(&prepared.auth)?;
        let command = self.journal.command(&prepared.operation_id)?;
        let operation = self.journal.operation(&prepared.operation_id)?;
        if !matches!(command.kind, p::MaintenanceKind::ConfigureLan { .. })
            || operation.command_digest != prepared.command_digest
            || command.actor_proof.proof_ref != prepared.auth.proof_ref.0
        {
            return Err(p::Error(
                "network dispatch is not bound to its original command".into(),
            ));
        }
        let mut config = self
            .product_config
            .as_ref()
            .ok_or_else(|| p::Error("managed product configuration is unavailable".into()))?
            .lock()
            .map_err(|_| p::Error("managed product configuration is unavailable".into()))?;
        if config.maintenance_owner()?.as_ref() != Some(&prepared.owner)
            || !self.network_change_preflight(&mut config)?
        {
            return Err(p::Error(
                "network dispatch owner or adapter is no longer available".into(),
            ));
        }
        if let p::MaintenanceKind::ConfigureLan {
            config_revision,
            validation_digest,
        } = &command.kind
        {
            if config.network_validation_digest(*config_revision, &prepared.owner)?
                != *validation_digest
            {
                return Err(p::Error(
                    "network dispatch validation is stale before native IO".into(),
                ));
            }
        }
        drop(config);
        self.execute_authorized(&prepared.auth, &prepared.operation_id, now)
    }
}
