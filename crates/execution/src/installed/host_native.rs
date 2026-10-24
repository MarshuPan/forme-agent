//! Service effects and receipt binding; independent from command admission.
use forme_protocol as p;

use super::{host_network::NetworkHealthCheck, NativeServiceManager, ProductHostBroker};

impl<M: NativeServiceManager> ProductHostBroker<M> {
    pub(super) fn apply_maintenance(
        &self,
        command: &p::MaintenanceCommand,
        operation: &p::MaintenanceOperation,
        now: p::Timestamp,
    ) -> p::Result<p::SchemaDigest> {
        use p::MaintenanceKind as Kind;
        match &command.kind {
            Kind::StartCore => {
                p::canonical_digest(&self.change_core(p::NativeServiceAction::Start)?)
            }
            Kind::StopCore => p::canonical_digest(&self.change_core(p::NativeServiceAction::Stop)?),
            Kind::RestartCore => {
                // A bad/unknown stop receipt cannot authorize a subsequent start.
                let stop = self.change_core(p::NativeServiceAction::Stop)?;
                let start = self.change_core(p::NativeServiceAction::Start)?;
                p::canonical_digest(&(stop, start))
            }
            Kind::ConfigureLan {
                config_revision,
                validation_digest,
            } => self.apply_network_change(
                command,
                operation,
                *config_revision,
                validation_digest,
                now,
            ),
            Kind::Inspect => {
                let status = self.services.inspect(p::BuiltinServiceId::Brain)?;
                if status.schema_version != p::SchemaVersion(1)
                    || status.service != p::BuiltinServiceId::Brain
                {
                    return Err(p::Error(
                        "native observation belongs to another service".into(),
                    ));
                }
                p::canonical_digest(&status)
            }
            _ => self
                .release_stager
                .as_ref()
                .ok_or_else(|| {
                    p::Error("maintenance operation requires its typed lifecycle worker".into())
                })?
                .execute(command, operation),
        }
    }

    fn apply_network_change(
        &self,
        command: &p::MaintenanceCommand,
        operation: &p::MaintenanceOperation,
        config_revision: u64,
        validation_digest: &p::SchemaDigest,
        now: p::Timestamp,
    ) -> p::Result<p::SchemaDigest> {
        let config = self
            .product_config
            .as_ref()
            .ok_or_else(|| p::Error("LAN configuration is unavailable".into()))?;
        let mut store = config
            .lock()
            .map_err(|_| p::Error("LAN configuration is unavailable".into()))?;
        let view = store.snapshot()?;
        let network = view.network.clone();
        let previous_network = store.active_network()?;
        let network_digest = network.validation_digest()?;
        let owner = view
            .owner
            .as_ref()
            .ok_or_else(|| p::Error("LAN configuration has no installed owner".into()))?;
        if view.active_revision != config_revision
            || store.network_validation_digest(config_revision, owner)? != *validation_digest
        {
            return Err(p::Error(
                "LAN configuration is stale; refresh before applying".into(),
            ));
        }
        let target_listener_digest = self.network_listener_digest(&network)?;
        let loopback = store.loopback_network();
        let loopback_listener_digest = self.network_listener_digest(&loopback)?;
        self.journal.record_network_progress(
            &operation.operation_id,
            self.journal.revision()?,
            p::ProductNetworkProgress {
                schema_version: p::PRODUCT_NETWORK_HEALTH_SCHEMA_VERSION,
                phase: p::ProductNetworkProgressPhase::TargetIntent,
                requested_listener_digest: target_listener_digest.clone(),
                active_bind: None,
                active_listener_digest: None,
                health_receipt_digest: None,
                failure_ref: None,
            },
            now,
        )?;
        let firewall =
            if network.bind.ip().is_loopback() && previous_network.bind.ip().is_loopback() {
                None
            } else {
                let receipt = self
                    .services
                    .reconcile_firewall(&previous_network, &network)?;
                if receipt.schema_version != p::SchemaVersion(1)
                    || receipt.bind != network.bind
                    || receipt.rule_ref
                        != network
                            .firewall
                            .as_ref()
                            .map(|value| value.rule_ref.clone())
                    || receipt.network_digest != network_digest
                    || receipt.enabled != !network.bind.ip().is_loopback()
                    || receipt.removed_rule_refs
                        != previous_network
                            .firewall
                            .as_ref()
                            .filter(|old| network.firewall.as_ref() != Some(old))
                            .map(|old| vec![old.rule_ref.clone()])
                            .unwrap_or_default()
                {
                    return Err(p::Error(
                    "native firewall result is unbound or unsettled; inspect the original operation"
                        .into(),
                ));
                }
                Some(receipt)
            };
        store.activate_network(
            command.operation_id.clone(),
            config_revision,
            owner,
            validation_digest,
        )?;
        drop(store);
        let stop = self.change_service(p::BuiltinServiceId::Web, p::NativeServiceAction::Stop)?;
        let start = self.change_service(p::BuiltinServiceId::Web, p::NativeServiceAction::Start)?;
        let health = {
            let config_guard = config
                .lock()
                .map_err(|_| p::Error("LAN configuration is unavailable".into()))?;
            self.probe_network_health(&config_guard, &network)?
        };
        match health {
            NetworkHealthCheck::Verified(health_receipt_digest) => {
                self.journal.record_network_progress(
                    &operation.operation_id,
                    self.journal.revision()?,
                    p::ProductNetworkProgress {
                        schema_version: p::PRODUCT_NETWORK_HEALTH_SCHEMA_VERSION,
                        phase: p::ProductNetworkProgressPhase::TargetVerified,
                        requested_listener_digest: target_listener_digest,
                        active_bind: Some(network.bind),
                        active_listener_digest: Some(self.network_listener_digest(&network)?),
                        health_receipt_digest: Some(health_receipt_digest.clone()),
                        failure_ref: None,
                    },
                    now,
                )?;
                p::canonical_digest(&(firewall, stop, start, health_receipt_digest))
            }
            NetworkHealthCheck::Untrusted => Err(p::Error(
                "Web health proof is not trusted; inspect the original operation".into(),
            )),
            NetworkHealthCheck::Unreachable => {
                let failure = p::canonical_digest(&(
                    "forme-product-web-health-unreachable-v1",
                    &operation.operation_id,
                    &target_listener_digest,
                ))?;
                self.journal.record_network_progress(
                    &operation.operation_id,
                    self.journal.revision()?,
                    p::ProductNetworkProgress {
                        schema_version: p::PRODUCT_NETWORK_HEALTH_SCHEMA_VERSION,
                        phase: p::ProductNetworkProgressPhase::FallbackIntent,
                        requested_listener_digest: target_listener_digest.clone(),
                        active_bind: None,
                        active_listener_digest: None,
                        health_receipt_digest: None,
                        failure_ref: Some(failure.clone()),
                    },
                    now,
                )?;
                let fallback_stop =
                    self.change_service(p::BuiltinServiceId::Web, p::NativeServiceAction::Stop)?;
                config
                    .lock()
                    .map_err(|_| p::Error("LAN configuration is unavailable".into()))?
                    .publish_loopback(&operation.operation_id, &failure)?;
                let fallback_start =
                    self.change_service(p::BuiltinServiceId::Web, p::NativeServiceAction::Start)?;
                let fallback_health = {
                    let config_guard = config
                        .lock()
                        .map_err(|_| p::Error("LAN configuration is unavailable".into()))?;
                    self.probe_network_health(&config_guard, &loopback)?
                };
                let NetworkHealthCheck::Verified(health_receipt_digest) = fallback_health else {
                    return Err(p::Error(
                        "loopback Web fallback could not be verified; inspect the original operation"
                            .into(),
                    ));
                };
                self.journal.record_network_progress(
                    &operation.operation_id,
                    self.journal.revision()?,
                    p::ProductNetworkProgress {
                        schema_version: p::PRODUCT_NETWORK_HEALTH_SCHEMA_VERSION,
                        phase: p::ProductNetworkProgressPhase::FallbackVerified,
                        requested_listener_digest: target_listener_digest,
                        active_bind: Some(loopback.bind),
                        active_listener_digest: Some(loopback_listener_digest),
                        health_receipt_digest: Some(health_receipt_digest.clone()),
                        failure_ref: None,
                    },
                    now,
                )?;
                p::canonical_digest(&(
                    firewall,
                    stop,
                    start,
                    failure,
                    fallback_stop,
                    fallback_start,
                    health_receipt_digest,
                ))
            }
        }
    }

    pub(super) fn change_service(
        &self,
        service: p::BuiltinServiceId,
        action: p::NativeServiceAction,
    ) -> p::Result<p::NativeReceipt> {
        let receipt = self.services.apply(p::ValidatedServiceChange {
            schema_version: p::SchemaVersion(1),
            service,
            action,
        })?;
        let expected = match action {
            p::NativeServiceAction::Start => p::NativeServiceState::Running,
            p::NativeServiceAction::Stop => p::NativeServiceState::Stopped,
        };
        if receipt.schema_version != p::SchemaVersion(1)
            || receipt.service != service
            || receipt.requested != action
            || receipt.observed != expected
        {
            return Err(p::Error(
                "native service result is unbound or unsettled; inspect the original operation"
                    .into(),
            ));
        }
        Ok(receipt)
    }

    pub(super) fn change_core(
        &self,
        action: p::NativeServiceAction,
    ) -> p::Result<p::NativeReceipt> {
        self.change_service(p::BuiltinServiceId::Brain, action)
    }
}
