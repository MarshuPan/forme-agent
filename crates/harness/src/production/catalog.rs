//! Request-scoped catalog hydration and immutable tool selection.
use super::*;

impl forme_context::ContextCatalogSource for ProductionContextCatalogSource {
    fn catalog(
        &self,
        request: &ContextHydrationRequest,
    ) -> p::Result<forme_context::ContextCatalogSnapshot> {
        crate::brain_workspaces::check_workspace(
            &self.store,
            &self.workspace,
            &request.workspace,
            &request.scope,
        )?;
        let mut resolve = self.resolve_context.clone();
        resolve.session = request.session.clone();
        if !scope_contains(&resolve.envelope.scope, &request.scope) {
            resolve.envelope.capability.capabilities.clear();
            resolve.envelope.capability.permissions.clear();
            resolve.policy_allowed_capabilities.clear();
            resolve.policy_allowed_providers.clear();
        }
        resolve.envelope.scope = request.scope.clone();
        let toolset = self.capabilities.resolve_toolset(&resolve)?;
        let visible_plugin_skills = toolset
            .items
            .iter()
            .filter_map(|item| match &item.capability {
                Capability::Skill(skill) => Some(skill.clone()),
                _ => None,
            })
            .collect::<BTreeSet<_>>();
        // Visibility is request-scoped. Mutating the shared skill registry
        // here would let concurrent hydration requests toggle one another's
        // catalog state.
        let plugin_skill_ids = self
            .plugin_skill_ids
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>();
        let current_plugin_skills = self
            .managed_plugins
            .as_ref()
            .map(|guard| guard.current_skills())
            .transpose()?;
        let metadata = self
            .skills
            .metadata(request.scope.clone())
            .into_iter()
            .filter(|skill| {
                !plugin_skill_ids.contains(&skill.id)
                    || (visible_plugin_skills.contains(&skill.id)
                        && current_plugin_skills
                            .as_ref()
                            .is_none_or(|current| current.contains(&skill.id)))
            })
            .collect::<Vec<_>>();
        let mut loaded_skill_bodies = Vec::new();
        for selected in &request.selected_skills {
            let skill = metadata
                .iter()
                .find(|metadata| &metadata.id == selected)
                .ok_or_else(|| p::Error("selected skill is not visible in this scope".into()))?;
            let body = self
                .skills
                .load_body(selected.clone(), forme_capabilities::LoadTrigger::Selected)?;
            loaded_skill_bodies.push(forme_context::LoadedSkillBody {
                schema_version: p::SchemaVersion(1),
                id: skill.id.clone(),
                body: body.0,
                trigger: p::SkillTriggerRef("selected".into()),
                provenance: skill_provenance(skill.trust),
            });
        }

        let mut tool_entries = Vec::new();
        let mut has_untrusted_schema = false;
        for item in toolset.items {
            let schema = match item.capability {
                Capability::Tool(tool) => self.static_tool_schemas.get(&tool).cloned(),
                Capability::McpTool(tool) => mcp_tool_schema(&self.mcp, &tool),
                Capability::AppApi(provider) => self.app_api_schemas.get(&provider).cloned(),
                _ => None,
            };
            if let Some(schema) = schema {
                if item
                    .sources
                    .iter()
                    .any(|source| source.0.starts_with("mcp:") || source.0.starts_with("app-api:"))
                {
                    has_untrusted_schema = true;
                }
                tool_entries.push(forme_context::ToolSchemaEntry {
                    schema_version: p::SchemaVersion(1),
                    tool: p::ToolRef(item.id.0),
                    schema,
                });
            }
        }
        tool_entries.sort_by(|left, right| left.tool.cmp(&right.tool));
        let skills_metadata = metadata
            .into_iter()
            .map(|skill| forme_context::SkillMetadata {
                schema_version: skill.schema_version,
                id: skill.id,
                summary: skill.summary,
                scope: skill.scope,
                version: skill.version,
                trust: skill.trust,
            })
            .collect();
        let schema = ToolSchema {
            schema_version: p::SchemaVersion(1),
            version: p::Version(1),
            entries: tool_entries,
            provenance: if has_untrusted_schema {
                untrusted_provenance()
            } else {
                trusted_provenance()
            },
        };
        let snapshot = self.snapshots.select(request, &schema)?;
        Ok(forme_context::ContextCatalogSnapshot {
            schema_version: p::SchemaVersion(1),
            rules: self.rules.clone(),
            skills_metadata,
            loaded_skill_bodies,
            tool_schema: snapshot.schema(),
        })
    }
}

fn mcp_tool_schema(mcp: &StdioMcpRegistry, tool: &p::McpToolRef) -> Option<String> {
    let tool = mcp.resolve_schema(tool.clone()).ok()?;
    let schema = tool.input_schema?;
    serde_json::to_string(&schema).ok()
}
