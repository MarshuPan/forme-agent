//! Service effects and receipt binding; independent from command admission.
use forme_protocol as p;

use super::{NativeServiceManager, ProductHostBroker};

impl<M: NativeServiceManager> ProductHostBroker<M> {
    pub(super) fn apply_maintenance(
        &self,
        command: &p::MaintenanceCommand,
        operation: &p::MaintenanceOperation,
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
            } => {
                let store = self
                    .product_config
                    .as_ref()
                    .ok_or_else(|| p::Error("LAN configuration is unavailable".into()))?;
                let mut store = store
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
                if view.active_revision != *config_revision
                    || store.network_validation_digest(*config_revision, owner)?
                        != *validation_digest
                {
                    return Err(p::Error(
                        "LAN configuration is stale; refresh before applying".into(),
                    ));
                }
                let firewall = if network.bind.ip().is_loopback()
                    && previous_network.bind.ip().is_loopback()
                {
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
                    *config_revision,
                    owner,
                    validation_digest,
                )?;
                drop(store);
                let stop =
                    self.change_service(p::BuiltinServiceId::Web, p::NativeServiceAction::Stop)?;
                let start =
                    self.change_service(p::BuiltinServiceId::Web, p::NativeServiceAction::Start)?;
                p::canonical_digest(&(firewall, stop, start))
            }
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
