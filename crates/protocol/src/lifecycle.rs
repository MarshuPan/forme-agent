use serde::{Deserialize, Serialize};

use crate::{
    AgentProfileRef, Budget, Constraint, EventId, IdempotencyKey, MemoryScope, ModelProfileRef,
    OutputRef, PolicyProfileRef, RunId, RunInput, RunStatus, SchemaVersion, SessionId, SessionRef,
    Source, StopReason, TurnId, WorkspaceId, WorkspaceRef,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Run {
    pub schema_version: SchemaVersion,
    pub run_id: RunId,
    pub source: Source,
    pub status: RunStatus,
    pub budget: Budget,
    pub stop_reason: Option<StopReason>,
    pub result_ref: Option<EventId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    pub schema_version: SchemaVersion,
    pub session_id: SessionId,
    pub workspace: WorkspaceRef,
    pub agent_profile: AgentProfileRef,
    pub policy_profile: PolicyProfileRef,
    pub model_profile: ModelProfileRef,
    pub memory_scope: MemoryScope,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Turn {
    pub schema_version: SchemaVersion,
    pub turn_id: TurnId,
    pub run_id: RunId,
    pub index: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunRequest {
    pub schema_version: SchemaVersion,
    pub source: Source,
    pub session: SessionRef,
    pub agent_profile: AgentProfileRef,
    pub input: RunInput,
    pub budget: Option<Budget>,
    pub idempotency_key: Option<IdempotencyKey>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunResult {
    pub schema_version: SchemaVersion,
    pub status: RunStatus,
    pub stop_reason: StopReason,
    pub outputs: Vec<OutputRef>,
    pub evidence_refs: Vec<EventId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Workspace {
    pub schema_version: SchemaVersion,
    pub workspace_id: WorkspaceId,
    pub charter: Option<WorkspaceCharter>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceCharter {
    pub schema_version: SchemaVersion,
    pub constraints: Vec<Constraint>,
}
