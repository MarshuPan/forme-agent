//! Select only verified metadata, then recheck the same source before planning/execution.
use super::*;

impl RunToolSnapshots {
    pub(super) fn add_managed(&self, snapshot: &mut RunToolSnapshot) -> p::Result<()> {
        let Some(source) = &self.managed_source else {
            return Ok(());
        };
        for configured in &self.managed_configs {
            if !configured.enabled
                || configured.trust != p::ProductSkillTrust::OwnerInput
                || configured.scope != snapshot.scope
            {
                continue;
            }
            let server = p::McpServerRef(configured.server_ref.clone());
            let Ok((settings, catalog)) = source.read_catalog(&server) else {
                continue;
            };
            if settings != *configured {
                continue;
            }
            let source_digest = p::canonical_digest(&(&settings, &catalog))?;
            let Ok(discovered) = self.registry.refresh(p::ProviderId(server.0.clone())) else {
                continue;
            };
            let mut selected = Vec::new();
            for tool in discovered.tools {
                let Ok(resolved) = self.registry.resolve_schema(tool.tool_ref.clone()) else {
                    continue;
                };
                let (Some(schema), Some(schema_digest)) =
                    (resolved.input_schema, resolved.schema_digest)
                else {
                    continue;
                };
                if !catalog
                    .tools
                    .iter()
                    .any(|t| t.name == tool.name && t.schema_digest == schema_digest)
                    || !configured.allowlist.tools.contains(&tool.name)
                {
                    return Err(invalid());
                }
                let reference = p::ToolRef(tool.capability_ref.0.clone());
                if snapshot.bindings.contains_key(&reference) {
                    return Err(invalid());
                }
                selected.push((
                    reference,
                    FormalToolBinding {
                        capability: tool.capability_ref,
                        backend: p::BackendKind::Mcp,
                        scope: configured.scope.clone(),
                        permissions: vec![p::PermissionRef("execute".into())],
                        risk: p::Risk::High,
                    },
                    McpPin {
                        reference: tool.tool_ref,
                        source: source_digest.clone(),
                        settings: configured.clone(),
                        schema_digest,
                    },
                    serde_json::to_string(&schema).map_err(|_| invalid())?,
                ));
            }
            // A concurrent disable/re-discovery cannot create a mixed snapshot.
            if p::canonical_digest(&source.read_catalog(&server)?)? != source_digest {
                return Err(invalid());
            }
            for (tool, binding, pin, schema) in selected {
                snapshot.entries.push(SchemaEntry {
                    tool: tool.clone(),
                    schema,
                });
                snapshot.bindings.insert(tool.clone(), binding);
                snapshot.managed.insert(tool, pin);
                snapshot.provenance.trust_tier = p::TrustTier::Untrusted;
                snapshot.provenance.actor = p::Actor::System;
            }
        }
        Ok(())
    }
    fn check_pin(&self, pin: &McpPin) -> p::Result<()> {
        let source = self.managed_source.as_ref().ok_or_else(invalid)?;
        let current = source.read_catalog(&p::McpServerRef(pin.settings.server_ref.clone()))?;
        if p::canonical_digest(&current)? != pin.source {
            return Err(invalid());
        }
        Ok(())
    }
    pub(crate) fn prepare_managed(
        &self,
        snapshot: &RunToolSnapshot,
        tool: &forme_models::ModelToolCall,
    ) -> p::Result<Option<p::ActionIntent>> {
        let Some(pin) = snapshot.managed.get(&tool.tool) else {
            return Ok(None);
        };
        self.check_pin(pin)?;
        // Managed refresh reads retained metadata only; it never starts stdio.
        self.registry
            .refresh(p::ProviderId(pin.settings.server_ref.clone()))
            .map_err(|_| invalid())?;
        let schema = self.registry.resolve_schema(pin.reference.clone())?;
        if schema.schema_digest.as_ref() != Some(&pin.schema_digest) {
            return Err(invalid());
        }
        // Current registry checks validate model data; authority still has to
        // match the original settings and schema before it can leave Harness.
        let intent = self
            .registry
            .try_prepare_call(pin.reference.clone(), tool.arguments.clone())?;
        self.recheck(snapshot, &intent)?;
        Ok(Some(intent))
    }
    pub(crate) fn recheck(
        &self,
        snapshot: &RunToolSnapshot,
        intent: &p::ActionIntent,
    ) -> p::Result<()> {
        if self.load(&snapshot.run)?.as_deref() != Some(snapshot) {
            return Err(invalid());
        }
        self.recheck_plugin(snapshot, intent)?;
        let key = p::ToolRef(intent.capability_ref.0.clone());
        let Some(pin) = snapshot.managed.get(&key) else {
            return if intent.backend_hint == p::BackendKind::Mcp
                && !matches!(intent.parameters, p::ActionParameters::McpDiscovery(_))
                && !self.managed_configs.is_empty()
            {
                Err(invalid())
            } else {
                Ok(())
            };
        };
        let binding = snapshot.bindings.get(&key).ok_or_else(invalid)?;
        let p::ActionParameters::Mcp {
            server,
            tool,
            schema_digest,
            transport,
            stdio,
            timeout,
            ..
        } = &intent.parameters
        else {
            return Err(invalid());
        };
        if intent.scope != binding.scope
            || intent.risk_hint != p::Risk::High
            || intent.expected_effect != p::ExpectedEffect::Outward
            || intent.requested_permissions != binding.permissions
            || intent.backend_hint != p::BackendKind::Mcp
            || intent.action_type != p::ActionType::Execute
            || server.0 != pin.settings.server_ref
            || format!("{}/{}", server.0, tool.0) != pin.reference.0
            || schema_digest.as_ref() != Some(&pin.schema_digest)
            || *transport != p::McpTransport::Stdio
            || stdio.command != pin.settings.program_ref
            || !stdio.args.is_empty()
            || !stdio.env.is_empty()
            || timeout != &pin.settings.timeout
        {
            return Err(invalid());
        }
        self.check_pin(pin)
    }
}
