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

    pub(super) fn change_core(
        &self,
        action: p::NativeServiceAction,
    ) -> p::Result<p::NativeReceipt> {
        let receipt = self.services.apply(p::ValidatedServiceChange {
            schema_version: p::SchemaVersion(1),
            service: p::BuiltinServiceId::Brain,
            action,
        })?;
        let expected = match action {
            p::NativeServiceAction::Start => p::NativeServiceState::Running,
            p::NativeServiceAction::Stop => p::NativeServiceState::Stopped,
        };
        if receipt.schema_version != p::SchemaVersion(1)
            || receipt.service != p::BuiltinServiceId::Brain
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
}
