//! Owner project supervision controls and read-only status projections.
use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectSupervisionState {
    pub schema_version: SchemaVersion,
    pub version: u64,
    pub enabled: bool,
}
impl Default for ProjectSupervisionState {
    fn default() -> Self {
        Self {
            schema_version: SchemaVersion(1),
            version: 0,
            enabled: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectSupervisionCommand {
    pub schema_version: SchemaVersion,
    pub command: IdempotencyKey,
    pub project: String,
    pub expected_project_digest: SchemaDigest,
    pub expected_version: u64,
    pub enabled: bool,
}
impl ProjectSupervisionCommand {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != SchemaVersion(1)
            || [
                &self.command.0,
                &self.project,
                &self.expected_project_digest.0,
            ]
            .iter()
            .any(|v| v.trim().is_empty() || v.len() > 512 || v.chars().any(char::is_control))
        {
            return Err(Error("project supervision command is invalid".into()));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProjectReviewStatus {
    Paused,
    ActiveRoute,
    Due,
    NotDue,
    Unverifiable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceProjectControlView {
    pub schema_version: SchemaVersion,
    pub project: String,
    pub workspace: WorkspaceRef,
    pub project_digest: SchemaDigest,
    pub supervision: ProjectSupervisionState,
    pub checkpoint: Option<ProjectCheckpoint>,
    pub active_routes: Vec<ExecutionRouteRef>,
    pub next_cadence: Option<Timestamp>,
    pub review: ProjectReviewStatus,
    pub reasons: Vec<String>,
    pub evidence: Vec<EventId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceControlSnapshot {
    pub schema_version: SchemaVersion,
    pub observed_at: Timestamp,
    pub revision: u64,
    pub workspaces: Vec<WorkspaceRef>,
    pub projects: Vec<WorkspaceProjectControlView>,
    pub stream_heads: BTreeMap<RunId, u64>,
}

impl OrchestrationLedger {
    pub fn project_supervision(&self, project: &str) -> ProjectSupervisionState {
        self.supervision.get(project).cloned().unwrap_or_default()
    }

    pub(super) fn change_supervision(
        &mut self,
        project: &str,
        digest: &SchemaDigest,
        expected: u64,
        enabled: bool,
    ) -> Result<()> {
        let spec = self
            .projects
            .get(project)
            .ok_or_else(|| Error("project is not declared".into()))?;
        let previous = self.project_supervision(project);
        if canonical_digest(spec)? != *digest || previous.version != expected {
            return Err(Error("project supervision expected state is stale".into()));
        }
        let version = expected
            .checked_add(1)
            .ok_or_else(|| Error("project supervision version exhausted".into()))?;
        self.supervision.insert(
            project.to_owned(),
            ProjectSupervisionState {
                schema_version: SchemaVersion(1),
                version,
                enabled,
            },
        );
        Ok(())
    }
}
