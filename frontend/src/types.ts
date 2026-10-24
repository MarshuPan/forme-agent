// Small views of the versioned Rust wire objects. Guards fail on unknown schema.
export type Json = null | boolean | number | string | Json[] | { [key: string]: Json };
export interface SessionView {
  schema_version: 1; account_ref: string; owner: string; login_name: string;
  auth_epoch: number; idle_expires_at: number; absolute_expires_at: number;
}
export interface SessionResponse { schema_version: 1; session: SessionView; csrf: string; revoke_operation?: string }
export interface WorkbenchSessionRecord {
  schema_version: 1; session: string; title: string; workspace: string | null;
  archived: boolean; created_at: number; updated_at: number;
}
export interface WorkbenchSessionList { schema_version: 1; sessions: WorkbenchSessionRecord[] }
export interface ArtifactUploadReceipt {
  schema_version: 1; content_ref: string; content_digest: string; scope: string;
  bytes: number; file_name: string; media_type: string;
}
export interface ArtifactCatalogEntry {
  schema_version: 1; content_ref: string; content_digest: string; scope: string;
  bytes: number; file_name: string | null; media_type: string | null;
}
export interface ArtifactCatalogView {
  schema_version: 1; session: string; digest: string; total: number;
  next_offset: number | null; artifacts: ArtifactCatalogEntry[];
}
export type WorkbenchSessionMutation =
  | { operation: 'rename'; title: string }
  | { operation: 'archive'; archived: boolean }
  | { operation: 'associate_workspace'; workspace: string | null };
export interface ControlProfile {
  schema_version: 1; owner: string;
  gateway: { schema_version: 1; surface: string; policy: string; model: string; toolset: string; workspace: string };
}
export interface RuntimeView {
  schema_version: 1; agent_profile: string; workspace: string; model_profiles: string[];
  service_states: { schema_version: 1; name: string; status: string; reason: string | null }[];
  digest: string;
}
export type RuntimeCheckStatus = 'ready' | 'disabled' | 'unsupported' | 'blocking';
export interface RuntimeCheck {
  schema_version: 1; name: string; status: RuntimeCheckStatus; reason: string | null;
}
export interface RuntimeDoctorView {
  schema_version: 1; checks: RuntimeCheck[]; blocking: boolean; digest: string;
}
export interface ProductModelProfile {
  schema_version: 1; id: string; provider: string; model: string; base_url: string;
  context_window: number; tool_use: boolean; strength: 'basic' | 'standard' | 'strong';
  input_microunits_per_million: number; output_microunits_per_million: number;
  requests_per_minute: number; tokens_per_minute: number; timeout_ms: number;
  fallback_group: string | null; default: boolean; tool_schema_versions: number[];
  credential_configured: boolean;
}
export interface ProductBrainPreferences {
  schema_version: 1; default_workspace: string;
  model_routing: { schema_version: 1; foreground: string | null; cognition: string | null };
}
export type ProductSkillTrust = 'untrusted' | 'owner_input';
export interface ProductSkillConfig {
  schema_version: 1; id: string; summary: string; scope: string; version: number;
  body: string; enabled: boolean; trust: ProductSkillTrust;
}
export interface ProductCapabilitySettings {
  schema_version: 1; skills: ProductSkillConfig[];
}
export interface ProductNetworkTls {
  schema_version: 1; certificate_path: string; private_key_path: string;
}
export interface ProductFirewallBinding { schema_version: 1; rule_ref: string }
export interface ProductNetworkConfig {
  schema_version: 1; bind: string; allowed_hosts: string[];
  tls: ProductNetworkTls | null; firewall: ProductFirewallBinding | null;
}
export interface ProductConfigView {
  schema_version: 1; install_ref: string; owner: string | null;
  active_revision: number; acknowledged_revision: number | null; pending_restart: boolean;
  configured: boolean; models: ProductModelProfile[]; issue_refs: string[];
  brain: ProductBrainPreferences; brain_source: 'installation_template' | 'managed_configuration';
  capabilities?: ProductCapabilitySettings;
  capabilities_source?: 'installation_template' | 'managed_configuration';
  network: ProductNetworkConfig; network_active: boolean;
  network_source: 'installation_template' | 'managed_configuration';
}
export interface ProductConfigValidation {
  schema_version: 1; revision: number; valid: boolean; restart_required: boolean;
  network_validation_digest: string; model_test_status: 'NotRun'; issue_refs: string[];
}
export interface ProductCoreActivation {
  schema_version: 1; operation_id: string; expected_revision: number;
  action: 'start' | 'restart'; requested_deadline: number;
}
export interface ProductOperation {
  schema_version: 1; operation_id: string; stage: string; pending_stage: string | null;
}
export interface ProductCoreActivationResult {
  schema_version: 1; operation: ProductOperation; configuration: ProductConfigView;
  service: { schema_version: 1; service: string; state: string };
}
export interface ProductModelProfilePatch {
  schema_version: 1; id: string; provider: string; model: string; base_url: string;
  credential?: string; context_window: number; tool_use: boolean; strength: 'basic' | 'standard' | 'strong';
  input_microunits_per_million: number; output_microunits_per_million: number;
  requests_per_minute: number; tokens_per_minute: number; timeout_ms: number;
  fallback_group: string | null; default: boolean; tool_schema_versions: number[];
}
export interface RunSummary {
  schema_version: 1; run: string; session: string; workspace: string | null;
  source: string; status: 'Accepted' | 'Running' | 'Waiting' | 'Complete' | 'Aborted' | 'Failed' | 'Limited';
  last_stream_seq: number; result: Json;
}
export type RunPendingKind =
  | { kind: 'ApprovalWait'; approval: string; checkpoint: string }
  | { kind: 'ToolInterrupt'; action: string; checkpoint: string; probe_hint: string }
  | { kind: 'Handoff'; target: string; checkpoint: string; reason: string }
  | { kind: 'ResolutionUnknown'; checkpoint: string; command_digest: string | null; reason: 'ClaimWithoutResult' | 'ActionWithoutRunTerminal' | 'HandoffContinuation' | 'ReusedHandoffTarget'; replay_command: RunControlResolutionCommand | null };
export interface RunPendingView { schema_version: 1; run: string; snapshot_upper_bound: number; pending: RunPendingKind | null }
export type RunControlResolutionCommand =
  | { ResolveToolOutcome: { command: string; resolution: { schema_version: 1; action: string; outcome: 'Completed' | 'Failed' | 'Cancelled'; result_ref: string | null; failure_ref: string | null; reason: string | null; summary: string } } }
  | { ResolveHandoff: { command: string; resolution: { schema_version: 1; target: string; accepted: boolean; result: string | null } } };
export interface BrainEvent {
  schema_version: 1; event_id: string; run_id: string; stream_seq: number; kind: string; payload: Json;
}
export interface PendingApproval {
  schema_version: 1; run: string; session: string; approval_id: string;
  action_summary: string; risk_level: string; scope: string; requested_permissions: string[];
  normalized_action_digest: string; affected_resources: string[]; rollback_boundary: Json;
  expires_at: number; choices: string[]; plan_digest: string; policy_version: number; tool_schema_version: number;
}
export interface RunCommand {
  schema_version: 1; source: 'UserTurn'; session: string; agent_profile: string;
  input: string; budget: null; idempotency_key: string;
}
export interface DataCandidate {
  schema_version: 1; candidate_ref: string; available: boolean; selected: boolean;
  [key: string]: Json;
}
export interface TaskSignature {
  schema_version: 1;
  partition: string;
  dependency: string;
  verification: string;
  clarity: string;
  extent: string;
  reversibility: string;
  anchors: string[];
}
export interface WorkspaceProject {
  schema_version: 1;
  reference: string;
  workspace: string;
  goal: string;
  done: string;
  summary: string;
  signature: TaskSignature;
  requirements: Json[];
  scope: string;
  capabilities: string[];
  model: string;
  budget_units: number;
  node_units: number;
  cadence_ms: number;
  deadline: number | null;
}
export interface OrchestrationLedger {
  schema_version: 1;
  revision: number;
  projects: Record<string, WorkspaceProject>;
  routes: Record<string, Json>;
  checkpoints: Record<string, Json>;
  commands: Record<string, string>;
  supervision?: Record<string, ProjectSupervisionState>;
}
export interface ProjectSupervisionState { schema_version: 1; version: number; enabled: boolean }
export interface ProjectSupervisionCommand {
  schema_version: 1; command: string; project: string; expected_project_digest: string; expected_version: number; enabled: boolean;
}
export interface WorkspaceProjectControlView {
  schema_version: 1; project: string; workspace: string; project_digest: string; supervision: ProjectSupervisionState;
  checkpoint: { schema_version: 1; route: string; input_digest: string; at: number } | null;
  active_routes: string[]; next_cadence: number | null;
  review: 'Paused' | 'ActiveRoute' | 'Due' | 'NotDue' | 'Unverifiable'; reasons: string[]; evidence: string[];
}
export interface WorkspaceControlSnapshot {
  schema_version: 1; observed_at: number; revision: number; workspaces: string[];
  projects: WorkspaceProjectControlView[]; stream_heads: Record<string, number>;
}
export type ProjectCommand =
  | { operation: 'register'; command: string; project: WorkspaceProject }
  | { operation: 'revise'; command: string; project: WorkspaceProject; expected_digest: string }
  | { operation: 'plan'; command: string; project: string; signature: TaskSignature; input_digest: string }
  | { operation: 'execute'; route: Json }
  | { operation: 'cancel'; command: string; route: Json };
export interface WorkspaceCharterRecord {
  schema_version: 1;
  workspace: string;
  version: number;
  goals: string[];
  constraints: string[];
  prohibitions: string[];
  done_contract: string | null;
  review_cadence: number | null;
  actor: string;
  digest: string;
}
export interface WorkspaceCharterChangedPayload {
  charter: WorkspaceCharterRecord;
  expected_version: number;
  committed_version: number;
}
export interface CandidateReviewCommand {
  schema_version: 1;
  run: string;
  candidate: string;
  expected_state: string;
  decision: string;
  actor: 'Owner';
  evidence: string[];
  retraction: Json | null;
}
export interface ScheduleCommand {
  schema_version: 1;
  intention: Json;
  session: string;
  envelope: Json;
  budget: string;
}
export interface ScheduledJob {
  schema_version: 1;
  intention: Json;
  binding: Json;
  lease_until: number | null;
  run: string | null;
  run_status: string | null;
  manual_review: boolean;
}
export function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}
export function versioned(value: unknown): asserts value is Record<string, unknown> & { schema_version: 1 } {
  if (!isRecord(value) || value.schema_version !== 1) throw new Error('服务协议版本不兼容，请更新页面与服务。');
}
export const isFinished = (status: RunSummary['status']) => ['Complete', 'Aborted', 'Failed', 'Limited'].includes(status);
