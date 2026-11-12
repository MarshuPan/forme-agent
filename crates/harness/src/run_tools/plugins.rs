//! A plugin slot can only alias authority already present in this run.
use super::*;
use forme_capabilities::{
    CapabilityRegistry, InMemoryCapabilityRegistry, LocalPluginRegistry, PluginRuntimeState,
};

pub(crate) struct ManagedPluginTools {
    pub(crate) sources: crate::managed_config::ManagedPluginSources,
    pub(crate) registry: Arc<LocalPluginRegistry>,
    pub(crate) capabilities: Arc<InMemoryCapabilityRegistry>,
    pub(crate) resolve: p::ResolveContext,
    pub(crate) generations: BTreeMap<p::PluginRef, u64>,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PluginPin {
    settings: p::ProductPluginConfig,
    slot: p::ProductPluginToolSlot,
}
impl ManagedPluginTools {
    fn visible(
        &self,
        snapshot: &RunToolSnapshot,
        slot: &p::ProductPluginToolSlot,
    ) -> p::Result<bool> {
        let mut context = self.resolve.clone();
        if !p::charter_scope_contains(&context.envelope.scope, &snapshot.scope) {
            return Ok(false);
        }
        context.session = snapshot.session.clone();
        context.envelope.scope = snapshot.scope.clone();
        let items = self.capabilities.resolve_toolset(&context)?.items;
        Ok(std::iter::once(&slot.id.0)
            .chain(slot.hooks.iter().map(|hook| &hook.id.0))
            .all(|id| items.iter().any(|item| item.id.0 == *id)))
    }
    fn active(&self, id: &p::PluginRef) -> bool {
        self.registry.status(id).is_some_and(|s| {
            s.state == PluginRuntimeState::Active && self.generations.get(id) == Some(&s.generation)
        })
    }
}
impl RunToolSnapshots {
    pub(super) fn add_plugins(&self, snapshot: &mut RunToolSnapshot) -> p::Result<()> {
        let Some(plugins) = &self.plugins else {
            return Ok(());
        };
        for source in plugins.sources.current_sources()? {
            if source.scope != snapshot.scope
                || !plugins.active(&p::PluginRef(source.plugin_ref.clone()))
            {
                continue;
            }
            for slot in &source.tool_slots {
                if !plugins.visible(snapshot, slot)? {
                    continue;
                }
                let Some(binding) = snapshot.bindings.get(&slot.binding).cloned() else {
                    continue;
                };
                let Some(schema) = snapshot
                    .entries
                    .iter()
                    .find(|e| e.tool == slot.binding)
                    .map(|e| e.schema.clone())
                else {
                    continue;
                };
                if snapshot.bindings.contains_key(&slot.id)
                    || snapshot.entries.iter().any(|e| e.tool == slot.id)
                {
                    return Err(invalid());
                }
                let settings = plugins.sources.settings(&source)?;
                plugins.sources.require_source(&settings)?;
                snapshot.entries.push(SchemaEntry {
                    tool: slot.id.clone(),
                    schema,
                });
                snapshot.bindings.insert(slot.id.clone(), binding);
                if let Some(pin) = snapshot.managed.get(&slot.binding).cloned() {
                    snapshot.managed.insert(slot.id.clone(), pin);
                }
                snapshot.plugin_pins.insert(
                    slot.id.clone(),
                    PluginPin {
                        settings,
                        slot: slot.clone(),
                    },
                );
            }
        }
        if !snapshot.plugin_pins.is_empty() {
            snapshot.schema_version = p::SchemaVersion(2);
            snapshot.provenance.trust_tier = p::TrustTier::Untrusted;
        }
        Ok(())
    }
    fn plugin(
        &self,
        snapshot: &RunToolSnapshot,
        pin: &PluginPin,
    ) -> p::Result<&ManagedPluginTools> {
        let plugins = self.plugins.as_ref().ok_or_else(invalid)?;
        plugins.sources.require_source(&pin.settings)?;
        if !plugins.active(&p::PluginRef(pin.settings.plugin_ref.clone()))
            || !plugins.visible(snapshot, &pin.slot)?
        {
            return Err(invalid());
        }
        Ok(plugins)
    }
    pub(crate) fn transform_plugin(
        &self,
        snapshot: &RunToolSnapshot,
        tool: &p::ToolRef,
        mut intent: p::ActionIntent,
    ) -> p::Result<p::ActionIntent> {
        let Some(pin) = snapshot.plugin_pins.get(tool) else {
            return Ok(intent);
        };
        let plugin = self.plugin(snapshot, pin)?;
        for hook in &pin.slot.hooks {
            intent = plugin.registry.apply_hook(hook.id.clone(), intent)?.intent;
        }
        self.check_plugin_binding(snapshot, pin, &intent)?;
        Ok(intent)
    }
    fn check_plugin_binding(
        &self,
        snapshot: &RunToolSnapshot,
        pin: &PluginPin,
        intent: &p::ActionIntent,
    ) -> p::Result<()> {
        let target = snapshot
            .bindings
            .get(&pin.slot.binding)
            .ok_or_else(invalid)?;
        if intent.capability_ref != target.capability
            || intent.backend_hint != target.backend
            || intent.scope != target.scope
            || intent.risk_hint != target.risk
            || intent.requested_permissions != target.permissions
        {
            return Err(invalid());
        }
        Ok(())
    }
    pub(super) fn recheck_plugin(
        &self,
        snapshot: &RunToolSnapshot,
        intent: &p::ActionIntent,
    ) -> p::Result<()> {
        if snapshot.plugin_pins.is_empty() {
            return Ok(());
        }
        for event in self.store.read_run(snapshot.run.clone()) {
            let p::EventPayload::ToolCallProposed(proposal) = event?.payload else {
                continue;
            };
            let action = p::ActionId(format!(
                "formal-tool:{}",
                p::canonical_digest(&(&snapshot.run, &proposal.call_id))?.0
            ));
            if action != intent.intent_id {
                continue;
            }
            let Some(pin) = snapshot.plugin_pins.get(&proposal.tool) else {
                return Ok(());
            };
            let plugins = self.plugin(snapshot, pin)?;
            self.check_plugin_binding(snapshot, pin, intent)?;
            let mut checked = intent.clone();
            for hook in pin
                .slot
                .hooks
                .iter()
                .filter(|h| h.point == p::ProductPluginHookPoint::BeforeExecution)
            {
                checked = plugins
                    .registry
                    .apply_hook(hook.id.clone(), checked)?
                    .intent;
            }
            if checked != *intent {
                return Err(invalid());
            }
            return Ok(());
        }
        Ok(())
    }
}
