//! Assemble only the declared local recipients and their isolated backend.
use super::*;
use crate::managed_config::ManagedChannels;
struct Contents {
    store: SqliteEventStore,
    scope: p::Scope,
}
impl ContentResolver for Contents {
    fn resolve(&self, reference: &p::ContentRef) -> p::Result<Vec<u8>> {
        if self.store.has_pending_data_cleanup()? {
            return Err(invalid());
        }
        let material =
            crate::production_artifact_store::read_material(&self.store, reference, &self.scope)?;
        let text = material.text.ok_or_else(invalid)?;
        if text.len() as u64 > p::MANAGED_CHANNEL_MAX_BYTES {
            return Err(invalid());
        }
        Ok(text.into_bytes())
    }
}
pub(super) fn register(
    assembly: &mut CapabilityAssembly,
    channels: Option<&ManagedChannels>,
    config: &ProductionRuntimeConfig,
    store: SqliteEventStore,
) -> p::Result<()> {
    let Some(channels) = channels else {
        return Ok(());
    };
    channels.current()?;
    if !config.capabilities.app_api.is_empty() {
        return Err(invalid());
    }
    let mut entries = Vec::new();
    let mut timeout = config.capabilities.timeout.0.min(60_000);
    let mut output = config
        .capabilities
        .output_budget_bytes
        .min(p::MANAGED_CHANNEL_MAX_BYTES);
    for source in &channels.ingress.sources {
        let delivery = source.delivery.as_ref().ok_or_else(invalid)?;
        if assembly.bindings.contains_key(&delivery.capability_ref) {
            return Err(invalid());
        }
        timeout = timeout.min(delivery.timeout.0);
        output = output.min(delivery.max_output_bytes);
        let capability = Capability::AppApi(p::ProviderId(source.connector_ref.clone()));
        entries.push(CapabilityDescriptor {
            schema_version: p::SchemaVersion(1),
            id: delivery.capability_ref.clone(),
            capability: capability.clone(),
            scope: source.scope.clone(),
            permissions: vec![delivery.permission.clone()],
            risk: Some(p::Risk::High),
            enabled: true,
        });
        add_binding(
            &mut assembly.bindings,
            delivery.capability_ref.clone(),
            capability,
            vec![delivery.permission.clone()],
            source.scope.clone(),
            p::Risk::High,
            p::BackendKind::AppApi,
        );
        assembly
            .resolve_context
            .envelope
            .capability
            .capabilities
            .push(delivery.capability_ref.clone());
        assembly
            .resolve_context
            .envelope
            .capability
            .permissions
            .push(delivery.permission.clone());
        assembly
            .resolve_context
            .policy_allowed_capabilities
            .push(delivery.capability_ref.clone());
        assembly.app_api_limits.push(AppApiConnectorLimit {
            schema_version: p::SchemaVersion(1),
            connector: p::ProviderId(source.connector_ref.clone()),
            base_url: delivery.endpoint.clone(),
            schema_digest: delivery.schema_digest.clone(),
            credential_ref: Some(p::SecretRef(delivery.credential_ref.clone())),
            allowed_mutations: vec![p::AppApiMutationMethod::Post],
            max_timeout: delivery.timeout,
        });
        assembly
            .service_states
            .push(RuntimeServiceState::ready(format!(
                "channel:{}",
                source.channel_ref
            )));
    }
    let backend = WorkerActionBackend::from_binding(
        p::BackendKind::AppApi,
        &channels.binding,
        p::VerifiedPrincipal(config.owner.0.clone()),
        OutputBudget::strict(output),
        p::DurationMs(timeout),
    )?
    .with_channel_delivery(
        channels.binding.channel_installation_ref()?,
        channels.ingress.sources.clone(),
        Arc::new(Contents {
            store,
            scope: p::Scope(config.workspace.0.clone()),
        }),
    )?;
    assembly.backends.register(Arc::new(backend))?;
    assembly.capabilities.index(CapabilitySource {
        schema_version: p::SchemaVersion(1),
        source_ref: p::CapabilitySourceRef("installed:channel-delivery".into()),
        trust: p::TrustTier::VerifiedProcess,
        entries,
    })?;
    assembly.network_allowed = true;
    assembly.rechecker = Arc::new(Rechecker {
        inner: assembly.rechecker.clone(),
        channels: channels.clone(),
    });
    Ok(())
}
struct Rechecker {
    inner: Arc<dyn forme_capabilities::ExecutionCapabilityRechecker + Send + Sync>,
    channels: ManagedChannels,
}
impl forme_capabilities::ExecutionCapabilityRechecker for Rechecker {
    fn recheck(
        &self,
        intent: &p::ActionIntent,
        context: &forme_policy::PolicyContext,
    ) -> p::Result<()> {
        if intent.backend_hint == p::BackendKind::AppApi {
            self.channels.recheck(intent)
        } else {
            self.inner.recheck(intent, context)
        }
    }
}
fn invalid() -> p::Error {
    p::Error("installed channel composition or protected message is unavailable".into())
}
