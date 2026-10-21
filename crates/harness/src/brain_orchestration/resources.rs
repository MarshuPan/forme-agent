use crate::production::ProductionRuntime;
use forme_capabilities::{CapabilityRegistry, Toolset};
use forme_protocol as p;
use std::path::PathBuf;

impl ProductionRuntime {
    pub fn project_node_directory(
        &self,
        route: &p::RouteDeclaration,
        node: &str,
        attempt: u32,
    ) -> p::Result<PathBuf> {
        let root = &self.config().data.artifact_root;
        std::fs::create_dir_all(root)
            .map_err(|_| p::Error("workspace artifact directory creation failed".into()))?;
        let root = root
            .canonicalize()
            .map_err(|_| p::Error("workspace artifact directory cannot be resolved".into()))?;
        let mut path = root.clone();
        for part in [
            "workspaces".into(),
            p::canonical_digest(&route.route)?.0.replace(':', "-"),
            p::canonical_digest(&(node, attempt))?.0.replace(':', "-"),
        ] {
            path.push(part);
            if std::fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink()) {
                return Err(p::Error("child workspace cannot traverse a link".into()));
            }
            std::fs::create_dir_all(&path)
                .map_err(|_| p::Error("child workspace creation failed".into()))?;
            if !path.canonicalize().is_ok_and(|p| p == path) {
                return Err(p::Error(
                    "child workspace contains a directory alias".into(),
                ));
            }
        }
        Ok(path)
    }

    pub(super) fn node_toolset(&self, spec: &p::WorkNode) -> p::Result<Toolset> {
        self.resolve_project_tools(&spec.capabilities, &spec.scope)
    }

    pub(super) fn project_toolset(&self, spec: &p::WorkspaceProject) -> p::Result<Toolset> {
        self.resolve_project_tools(&spec.capabilities, &spec.scope)
    }

    fn resolve_project_tools(
        &self,
        capabilities: &[p::CapabilityRef],
        scope: &p::Scope,
    ) -> p::Result<Toolset> {
        let mut envelope = self
            .harness()
            .governance
            .envelope
            .clone()
            .ok_or_else(|| p::Error("project has no capability delegation".into()))?;
        envelope.scope = scope.clone();
        envelope.capability.capabilities = capabilities.to_vec();
        self.services()
            .capabilities
            .resolve_toolset(&p::ResolveContext {
                schema_version: p::SchemaVersion(1),
                session: p::SessionId("session:owner-project-resources".into()),
                toolset: p::ToolsetRef("toolset:owner-project-resources".into()),
                envelope,
                policy_allowed_providers: Vec::new(),
                policy_allowed_capabilities: capabilities.to_vec(),
            })
    }
}
