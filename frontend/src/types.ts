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
export function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}
export function versioned(value: unknown): asserts value is Record<string, unknown> & { schema_version: 1 } {
  if (!isRecord(value) || value.schema_version !== 1) throw new Error('服务协议版本不兼容，请更新页面与服务。');
}
export const isFinished = (status: RunSummary['status']) => ['Complete', 'Aborted', 'Failed', 'Limited'].includes(status);
