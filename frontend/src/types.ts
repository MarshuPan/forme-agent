// Small views of the versioned Rust wire objects. Guards fail on unknown schema.
export type Json = null | boolean | number | string | Json[] | { [key: string]: Json };
export interface SessionView {
  schema_version: 1; account_ref: string; owner: string; login_name: string;
  auth_epoch: number; idle_expires_at: number; absolute_expires_at: number;
}
export interface SessionResponse { schema_version: 1; session: SessionView; csrf: string; revoke_operation?: string }
export interface ControlProfile {
  schema_version: 1; owner: string;
  gateway: { schema_version: 1; surface: string; policy: string; model: string; toolset: string; workspace: string };
}
export interface RuntimeView {
  schema_version: 1; agent_profile: string; workspace: string; model_profiles: string[];
  service_states: { schema_version: 1; name: string; status: string; reason: string | null }[];
  digest: string;
}
export interface RunSummary {
  schema_version: 1; run: string; session: string; workspace: string | null;
  source: string; status: 'Accepted' | 'Running' | 'Waiting' | 'Complete' | 'Aborted' | 'Failed' | 'Limited';
  last_stream_seq: number; result: Json;
}
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
