//! Immutable, protected tool material for one run. This is not a provider registry.
use crate::{formal_tools::FormalToolBinding, GovernanceConfig, ReactiveHarness};
use forme_capabilities::StdioMcpRegistry;
use forme_context::{ContextHydrationRequest, ToolSchema};
use forme_protocol as p;
use forme_store::{EventStore, SqliteEventStore};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, OnceLock},
};

mod managed;
mod persistence;
mod plugins;
pub(crate) use plugins::ManagedPluginTools;
#[cfg(test)]
mod tests;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SchemaEntry {
    tool: p::ToolRef,
    schema: String,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct McpPin {
    reference: p::McpToolRef,
    source: p::SchemaDigest,
    settings: p::ProductMcpServerConfig,
    schema_digest: p::SchemaDigest,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RunToolSnapshot {
    schema_version: p::SchemaVersion,
    run: p::RunId,
    session: p::SessionId,
    workspace: p::WorkspaceRef,
    scope: p::Scope,
    composition: p::SchemaDigest,
    envelope: p::AutonomyEnvelope,
    entries: Vec<SchemaEntry>,
    provenance: p::Provenance,
    bindings: BTreeMap<p::ToolRef, FormalToolBinding>,
    managed: BTreeMap<p::ToolRef, McpPin>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    plugin_pins: BTreeMap<p::ToolRef, plugins::PluginPin>,
}

pub(crate) struct RunToolSnapshots {
    pub(crate) store: Arc<SqliteEventStore>,
    pub(crate) owner: p::VerifiedPrincipal,
    pub(crate) composition: OnceLock<p::SchemaDigest>,
    pub(crate) access: Mutex<()>,
    pub(crate) bindings: BTreeMap<p::ToolRef, FormalToolBinding>,
    pub(crate) envelope: p::AutonomyEnvelope,
    pub(crate) registry: Arc<StdioMcpRegistry>,
    pub(crate) managed_source: Option<Arc<dyn forme_capabilities::ManagedMcpCatalogSource>>,
    pub(crate) managed_configs: Vec<p::ProductMcpServerConfig>,
    pub(crate) plugins: Option<ManagedPluginTools>,
}

fn invalid() -> p::Error {
    p::Error("original run tool snapshot is missing, changed or outside this context".into())
}

impl RunToolSnapshot {
    pub(crate) fn reference(&self) -> p::Result<p::ToolsetRef> {
        let base = p::canonical_digest(&(
            "forme-run-toolset-v1",
            self.schema_version,
            &self.workspace,
            &self.scope,
            &self.composition,
            &self.envelope,
            &self.entries,
            &self.provenance,
            &self.bindings,
            &self.managed,
        ))?;
        let digest = if self.plugin_pins.is_empty() {
            base
        } else {
            p::canonical_digest(&("forme-run-plugin-toolset-v2", base, &self.plugin_pins))?
        };
        Ok(p::ToolsetRef(format!("toolset:run:{}", digest.0)))
    }
    pub(crate) fn binding(&self, tool: &p::ToolRef) -> Option<&FormalToolBinding> {
        self.bindings.get(tool)
    }
    pub(crate) fn schema(&self) -> ToolSchema {
        ToolSchema {
            schema_version: p::SchemaVersion(1),
            version: p::Version(1),
            entries: self
                .entries
                .iter()
                .map(|e| forme_context::ToolSchemaEntry {
                    schema_version: p::SchemaVersion(1),
                    tool: e.tool.clone(),
                    schema: e.schema.clone(),
                })
                .collect(),
            provenance: self.provenance.clone(),
        }
    }
    pub(crate) fn governance(&self, base: &GovernanceConfig) -> GovernanceConfig {
        let mut value = base.clone();
        value.visible_capabilities = self
            .bindings
            .values()
            .map(|b| b.capability.clone())
            .collect();
        value.granted_permissions = self.envelope.capability.permissions.clone();
        value.envelope = Some(self.envelope.clone());
        if let Some(delegation) = &mut value.delegation {
            delegation.envelope = self.envelope.clone();
        }
        // Keep every base layer, including Deny. Availability adds Ask only.
        for (tool, pin) in &self.managed {
            let binding = &self.bindings[tool];
            value.mcp_allowlist.push(pin.reference.0.clone());
            value.allowed_scopes.push(binding.scope.clone());
            value.layers.push(forme_policy::PolicyLayer {
                schema_version: p::SchemaVersion(1),
                source: forme_policy::PolicyLayerSource::User,
                rules: vec![forme_policy::PolicyRule {
                    schema_version: p::SchemaVersion(1),
                    matcher: forme_policy::ActionMatcher {
                        backend: Some(p::BackendKind::Mcp),
                        capability: Some(binding.capability.clone()),
                        action_type: Some(p::ActionType::Execute),
                        parameters: forme_policy::ArgMatcher::Any,
                    },
                    effect: p::PolicyDecision::Ask,
                    scope: binding.scope.clone(),
                }],
            });
        }
        value.sandbox_available |= !self.managed.is_empty();
        value
    }
}

impl RunToolSnapshots {
    pub(crate) fn load(&self, run: &p::RunId) -> p::Result<Option<Arc<RunToolSnapshot>>> {
        let original = self.read(run)?;
        if let Some(original) = &original {
            if original.schema_version
                != p::SchemaVersion(if original.plugin_pins.is_empty() {
                    1
                } else {
                    2
                })
                || original.run != *run
                || self.composition.get() != Some(&original.composition)
            {
                return Err(invalid());
            }
            let mut accepted = 0;
            let mut bound = 0;
            for event in self.store.read_run(run.clone()) {
                match event?.payload {
                    p::EventPayload::RunAccepted(value) => {
                        accepted += 1;
                        if value.session_ref != original.session {
                            return Err(invalid());
                        }
                    }
                    p::EventPayload::SessionBound(value)
                        if value.toolset_ref.0.starts_with("toolset:run:") =>
                    {
                        bound += 1;
                        if value.toolset_ref != original.reference()?
                            || value.workspace != original.workspace
                            || value.composition_manifest.as_ref() != Some(&original.composition)
                        {
                            return Err(invalid());
                        }
                    }
                    _ => (),
                }
            }
            if accepted > 1 || bound > 1 {
                return Err(invalid());
            }
        }
        Ok(original.map(Arc::new))
    }
    pub(crate) fn select(
        &self,
        request: &ContextHydrationRequest,
        schema: &ToolSchema,
    ) -> p::Result<Arc<RunToolSnapshot>> {
        let _access = self.access.lock().map_err(|_| invalid())?;
        if let Some(original) = self.load(&request.run)? {
            if original.session != request.session
                || original.workspace != request.workspace
                || original.scope != request.scope
            {
                return Err(invalid());
            }
            return Ok(original);
        }
        // A deleted/missing original of an accepted run is never regenerated.
        if self.store.read_run(request.run.clone()).any(|e| {
            e.map_or(true, |e| {
                matches!(
                    e.kind,
                    p::EventKind::RunAccepted | p::EventKind::SessionBound
                )
            })
        }) {
            return Err(invalid());
        }
        // Startup resolution may already contain a managed server's catalog.
        // Rebuild those entries from one current source read below, otherwise a
        // restart either duplicates them or mixes old schemas with new pins.
        let managed_tool = |tool: &p::ToolRef| {
            self.managed_configs.iter().any(|server| {
                server
                    .allowlist
                    .tools
                    .iter()
                    .any(|name| tool.0 == format!("mcp:{}:{name}", server.server_ref))
            })
        };
        let mut snapshot = RunToolSnapshot {
            schema_version: p::SchemaVersion(1),
            run: request.run.clone(),
            session: request.session.clone(),
            workspace: request.workspace.clone(),
            scope: request.scope.clone(),
            composition: self.composition.get().ok_or_else(invalid)?.clone(),
            envelope: self.envelope.clone(),
            entries: schema
                .entries
                .iter()
                .filter(|entry| !managed_tool(&entry.tool))
                .map(|e| SchemaEntry {
                    tool: e.tool.clone(),
                    schema: e.schema.clone(),
                })
                .collect(),
            provenance: schema.provenance.clone(),
            bindings: self
                .bindings
                .iter()
                .filter(|(tool, binding)| {
                    !managed_tool(tool)
                        && schema.entries.iter().any(|e| &e.tool == *tool)
                        && p::charter_scope_contains(&binding.scope, &request.scope)
                })
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
            managed: BTreeMap::new(),
            plugin_pins: BTreeMap::new(),
        };
        snapshot.envelope.scope = request.scope.clone();
        if p::charter_scope_contains(&self.envelope.scope, &request.scope) {
            self.add_managed(&mut snapshot)?;
            self.add_plugins(&mut snapshot)?;
        }
        snapshot.envelope.capability.capabilities = snapshot
            .bindings
            .values()
            .map(|b| b.capability.clone())
            .collect();
        snapshot.envelope.capability.permissions = snapshot
            .bindings
            .values()
            .flat_map(|b| b.permissions.clone())
            .collect();
        snapshot.envelope.capability.permissions.sort();
        snapshot.envelope.capability.permissions.dedup();
        snapshot.entries.sort_by(|a, b| a.tool.cmp(&b.tool));
        self.write(&snapshot)?;
        self.load(&request.run)?.ok_or_else(invalid)
    }
}

impl ReactiveHarness {
    pub(crate) fn run_tool_snapshot(
        &self,
        run: &p::RunId,
    ) -> p::Result<Option<Arc<RunToolSnapshot>>> {
        self.tool_snapshots
            .as_ref()
            .map(|s| s.load(run)?.ok_or_else(invalid))
            .transpose()
    }
    pub(crate) fn record_toolset(&self, record: &crate::RunRecord) -> p::Result<p::ToolsetRef> {
        record
            .tools
            .as_ref()
            .map_or_else(|| Ok(self.config.toolset_ref.clone()), |s| s.reference())
    }
    pub(crate) fn validate_tool_binding(
        &self,
        run: &p::RunId,
        reference: &p::ToolsetRef,
    ) -> p::Result<()> {
        match &self.tool_snapshots {
            Some(s) => {
                let original = s.load(run)?.ok_or_else(invalid)?;
                if original.reference()? != *reference {
                    return Err(invalid());
                }
            }
            None if *reference != self.config.toolset_ref => return Err(invalid()),
            None => (),
        }
        Ok(())
    }
}
