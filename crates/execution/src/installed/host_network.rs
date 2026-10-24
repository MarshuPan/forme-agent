//! A single admitted network change, dispatched only after its response closes.
use ed25519_dalek::{Signature, VerifyingKey};
use forme_protocol as p;

use super::{HostAuth, NativeServiceManager, ProductHostBroker};

pub(super) enum NetworkHealthCheck {
    Verified(p::SchemaDigest),
    Unreachable,
    Untrusted,
}

/// In-process, non-cloneable dispatch authority. A journal entry alone cannot
/// recreate it after restart, response loss or a repeated POST.
pub struct PreparedOwnerNetwork {
    pub(super) auth: HostAuth,
    pub(super) operation_id: String,
    pub(super) command_digest: p::SchemaDigest,
    pub(super) owner: p::OwnerPrincipalRef,
}

impl<M: NativeServiceManager> ProductHostBroker<M> {
    pub(super) fn network_listener_digest(
        &self,
        network: &p::ProductNetworkConfig,
    ) -> p::Result<p::SchemaDigest> {
        p::product_network_listener_digest(
            network.bind,
            &network.allowed_hosts,
            network.tls.is_some(),
        )
    }

    pub(super) fn probe_network_health(
        &self,
        config: &super::ProductConfigStore,
        network: &p::ProductNetworkConfig,
    ) -> p::Result<NetworkHealthCheck> {
        let listener_digest = self.network_listener_digest(network)?;
        let mut nonce_bytes = [0_u8; 32];
        getrandom::fill(&mut nonce_bytes)
            .map_err(|_| p::Error("Web health probe entropy is unavailable".into()))?;
        let nonce = nonce_bytes
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let observation = self
            .services
            .probe_web_listener(network, &listener_digest, &nonce)?;
        let proof = match observation {
            p::ProductNetworkHealthObservation::Verified(proof) => proof,
            p::ProductNetworkHealthObservation::Unreachable => {
                return Ok(NetworkHealthCheck::Unreachable)
            }
            p::ProductNetworkHealthObservation::Untrusted => {
                return Ok(NetworkHealthCheck::Untrusted)
            }
        };
        let installation_digest =
            p::product_network_installation_digest(config.installation_ref())?;
        if proof.validate().is_err()
            || proof.installation_digest != installation_digest
            || proof.listener_digest != listener_digest
            || proof.nonce != nonce
        {
            return Ok(NetworkHealthCheck::Untrusted);
        }
        let key = VerifyingKey::from_bytes(&config.web_assertion_verifying_key())
            .map_err(|_| p::Error("Web health verification key is invalid".into()))?;
        let signature = Signature::from_slice(&proof.signature)
            .map_err(|_| p::Error("Web health signature is invalid".into()))?;
        if key
            .verify_strict(proof.signing_digest()?.0.as_bytes(), &signature)
            .is_err()
        {
            return Ok(NetworkHealthCheck::Untrusted);
        }
        Ok(NetworkHealthCheck::Verified(p::canonical_digest(&proof)?))
    }

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
        if !self.services.supports_web_health() {
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
