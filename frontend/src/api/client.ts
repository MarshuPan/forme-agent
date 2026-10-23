import { isRecord, versioned } from '../types.ts';
import type { ArtifactCatalogView } from '../types.ts';
import type { ProjectSupervisionCommand, WorkspaceControlSnapshot } from '../types.ts';
import type { ArtifactUploadReceipt, ControlProfile, DataCandidate, OrchestrationLedger, PendingApproval, ProductConfigView, ProductModelProfilePatch, ProjectCommand, RunCommand, RunSummary, RuntimeView, ScheduleCommand, ScheduledJob, SessionResponse, WorkbenchSessionList, WorkbenchSessionRecord, WorkbenchSessionMutation, WorkspaceCharterChangedPayload, WorkspaceCharterRecord } from '../types.ts';
import type { ProductConfigValidation, ProductCoreActivation, ProductCoreActivationResult, ProductOperation, RunPendingView } from '../types.ts';
import { isWorkbenchProposal } from '../workbench.ts';
import type { PendingWorkbenchProposals, WorkbenchProposal, WorkbenchReceipt, WorkbenchReview, WorkbenchSection, WorkbenchSnapshot } from '../workbench.ts';
import type { DataCommand, DataRead, DataReceipt, DataView } from '../data.ts';
import type { MaintenanceCommand, MaintenanceSnapshot } from '../maintenance.ts';
import { parseRuntimeDoctor } from '../runtime-diagnostics.ts';
import { parseMemoryMaterial } from '../memory-material.ts';
import type { WorkbenchItem } from '../workbench.ts';

export class ApiError extends Error {
  readonly status: number;
  readonly code: string;
  readonly retryAfterMs: number;
  constructor(status: number, code: string, message: string, retryAfterMs = 0) {
    super(message); this.status = status; this.code = code; this.retryAfterMs = retryAfterMs;
  }
}

export const newKey = (prefix = 'web') => `${prefix}:${crypto.randomUUID()}`;
const id = (value: string) => encodeURIComponent(value);

/** Only same-origin, allowlisted endpoints. No provider URL or internal token. */
export class WorkbenchClient {
  private csrf: string | null = null;
  private generation = 0;
  private readonly transport: typeof fetch;
  private readonly expired: () => void;
  constructor(transport = fetch, expired = () => {}) {
    // Browser fetch checks its receiver. Keep it detached from this client.
    this.transport = (...args) => transport(...args); this.expired = expired;
  }
  clear() { this.csrf = null; this.generation++; }
  private current(generation: number) {
    if (generation !== this.generation) throw new ApiError(401, 'SessionChanged', '登录状态已变化，请重新读取当前页面。');
  }

  private async request(path: string, method = 'GET', body?: unknown, key?: string): Promise<Response> {
    const generation = this.generation;
    const headers = new Headers({ Accept: 'application/json' });
    if (method !== 'GET') {
      if (!key) throw new Error('操作缺少稳定标识。');
      headers.set('Idempotency-Key', key);
      if (this.csrf) headers.set('X-Forme-Csrf', this.csrf);
      if (body !== undefined) headers.set('Content-Type', 'application/json');
    }
    let response: Response;
    try {
      response = await this.transport(`/api/v1/${path}`, {
        method, headers, credentials: 'same-origin', cache: 'no-store',
        body: body === undefined ? undefined : JSON.stringify(body), signal: AbortSignal.timeout(28_000),
      });
    } catch {
      // Mutation outcomes are unknown. Never create a new key or retry here.
      throw new ApiError(0, 'ConnectionLost', '连接中断；操作结果尚未确认，请使用原操作继续核实。');
    }
    this.current(generation);
    if (!response.ok) {
      const retry = response.headers.get('Retry-After');
      const seconds = retry !== null ? Number(retry) : NaN;
      const retryAfterMs = Number.isFinite(seconds) ? Math.max(0, seconds * 1000) : 0;
      let code = 'RequestFailed', message = `请求未完成 (${response.status})`;
      try {
        const problem: unknown = await response.json();
        if (isRecord(problem) && problem.schema_version === 1) {
          if (typeof problem.code === 'string') code = problem.code;
          if (typeof problem.safe_message === 'string') message = problem.safe_message;
        }
      } catch { /* Never display raw upstream error bodies. */ }
      this.current(generation);
      if (response.status === 401) { this.clear(); this.expired(); }
      throw new ApiError(response.status, code, message, retryAfterMs);
    }
    return response;
  }
  private async json<T>(path: string, method = 'GET', body?: unknown, key?: string): Promise<T> {
    const generation = this.generation;
    const response = await this.request(path, method, body, key);
    if (response.status === 204) return undefined as T;
    const value: unknown = await response.json();
    this.current(generation);
    if (isRecord(value)) versioned(value);
    if (Array.isArray(value)) for (const item of value) if (isRecord(item)) versioned(item);
    return value as T;
  }
  bootstrap() { return this.json<{ schema_version: 1; claimed: boolean; setup_needed: boolean }>('bootstrap/status'); }
  async session(): Promise<SessionResponse> {
    const view = await this.json<SessionResponse>('auth/session'); this.csrf = view.csrf; return view;
  }
  async login(login_name: string, password: string) {
    const challenge = await this.json<{ schema_version: 1; csrf: string; attempt_ref: string }>('auth/login-challenge');
    this.csrf = challenge.csrf;
    const result = await this.json<SessionResponse>('auth/login', 'POST', { schema_version: 1, login_name, password }, challenge.attempt_ref);
    this.csrf = result.csrf; return result;
  }
  async prove(proof: string, operation: string) {
    const result = await this.json<{ schema_version: 1; csrf: string }>('bootstrap/prove', 'POST', { schema_version: 1, proof }, operation);
    this.csrf = result.csrf; return result;
  }
  data() { return this.json<DataCandidate>('bootstrap/data'); }
  select(candidate_ref: string, operation: string) { return this.json<DataCandidate>('bootstrap/data', 'POST', { schema_version: 1, candidate_ref }, operation); }
  async claim(candidate_ref: string, login_name: string, password: string, operation: string) {
    const view = await this.json<SessionResponse>('bootstrap/claim', 'POST', { schema_version: 1, candidate_ref, login_name, password, operation_id: operation }, operation);
    this.csrf = view.csrf; return view;
  }
  async logout() { await this.request('auth/logout', 'POST', undefined, newKey('logout')); this.clear(); }
  workbenchSessions() { return this.json<WorkbenchSessionList>('sessions'); }
  createWorkbenchSession(title: string, workspace: string | null, operation = newKey('session')) {
    return this.json<WorkbenchSessionRecord>('sessions', 'POST', {
      schema_version: 1, command: operation, title, workspace,
    }, operation);
  }
  updateWorkbenchSession(session: string, expected_updated_at: number, mutation: WorkbenchSessionMutation, operation = newKey('session-update')) {
    return this.json<WorkbenchSessionRecord>(`sessions/${id(session)}`, 'PUT', {
      schema_version: 1, command: operation, session, expected_updated_at, mutation,
    }, operation);
  }
  async password(old_password: string, new_password: string, operation: string) {
    await this.request('auth/password', 'PUT', { schema_version: 1, old_password, new_password }, operation); this.clear();
  }
  productConfig() { return this.json<ProductConfigView>('product/config'); }
  saveProductConfig(expected_revision: number, models: ProductModelProfilePatch[], operation = newKey('config')) {
    return this.json<{ schema_version: 1; view: ProductConfigView; applied: boolean; operation_id: string | null }>(
      'product/config', 'POST', { schema_version: 1, operation_id: operation, expected_revision, models }, operation,
    );
  }
  validateProductConfig(revision: number, operation = newKey('config-check')) {
    return this.json<ProductConfigValidation>('product/config/validate', 'POST', { schema_version: 1, revision }, operation);
  }
  activateProductConfig(command: ProductCoreActivation, password: string) {
    return this.json<ProductCoreActivationResult>('product/config/activate', 'POST', { ...command, password }, command.operation_id);
  }
  productOperation(operation: string) { return this.json<ProductOperation>(`product/operations/${id(operation)}`); }
  maintenance() { return this.json<MaintenanceSnapshot>('product/maintenance'); }
  maintain(command: MaintenanceCommand, password: string) {
    return this.json<ProductOperation>('product/maintenance', 'POST', { ...command, password }, command.operation_id);
  }
  maintenanceOperation(operation: string) { return this.json<ProductOperation>(`product/maintenance/operations/${id(operation)}`); }
  profile() { return this.json<ControlProfile>('brain/profile'); }
  runtime() { return this.json<RuntimeView>('brain/runtime/manifest'); }
  async runtimeDoctor() { return parseRuntimeDoctor(await this.json<unknown>('brain/runtime/doctor')); }
  runs() { return this.json<RunSummary[]>('brain/runs'); }
  run(run: string) { return this.json<RunSummary>(`brain/runs/${id(run)}`); }
  runPending(run: string) { return this.json<RunPendingView>(`brain/runs/${id(run)}/pending`); }
  async controlRun(run: string, body: unknown, operation: string) {
    await this.request(`brain/runs/${id(run)}/control`, 'POST', body, operation);
  }
  submit(command: RunCommand) { return this.json<string>('brain/runs', 'POST', command, command.idempotency_key); }
  async events(run: string, after: number) { return (await this.request(`brain/runs/${id(run)}/events?after=${after}`)).text(); }
  async uploadArtifact(session: string, workspace: string | null, file: File, operation = newKey('artifact')) {
    if (file.size === 0 || file.size > 4 * 1024 * 1024) throw new Error('附件必须大于 0 且不超过 4 MiB。');
    const bytes = new Uint8Array(await file.arrayBuffer());
    let bytesHex = '';
    for (const byte of bytes) bytesHex += byte.toString(16).padStart(2, '0');
    return this.json<ArtifactUploadReceipt>('brain/artifacts/upload', 'POST', {
      schema_version: 1, command: operation, session, workspace,
      file_name: file.name, media_type: file.type || 'application/octet-stream', bytes_hex: bytesHex,
    }, operation);
  }
  async artifact(contentRef: string, session: string) {
    const response = await this.request(`brain/artifacts/${id(contentRef)}?session=${id(session)}`);
    return response.blob();
  }
  artifacts(session: string, offset = 0, snapshot: string | null = null) {
    return this.json<ArtifactCatalogView>('brain/artifacts/query', 'POST', {
      schema_version: 1, session, page: { schema_version: 1, offset, limit: 50, snapshot },
    }, newKey('artifact-query'));
  }
  answer(run: string) { return this.json<{ schema_version: 1; run: string; answer: string | null }>(`brain/runs/${id(run)}/answer`); }
  approvals(session: string) { return this.json<PendingApproval[]>(`brain/sessions/${id(session)}/approvals`); }
  async cancel(run: string, operation: string) {
    await this.request(`brain/runs/${id(run)}/control`, 'POST', { Cancel: { schema_version: 1, reason: 'reason:owner-web-cancel' } }, operation);
  }
  async approve(approval: PendingApproval, granted: boolean, operation: string) {
    const body = { ResolveApproval: {
      schema_version: 1, approval_id: approval.approval_id, outcome: granted ? 'Granted' : 'Denied',
      bound_plan_digest: approval.plan_digest, normalized_action_digest: approval.normalized_action_digest,
      affected_resources: approval.affected_resources, policy_version: approval.policy_version,
      tool_schema_version: approval.tool_schema_version, nonce: operation, use_by: approval.expires_at,
    } };
    await this.request(`brain/runs/${id(approval.run)}/control`, 'POST', body, operation);
  }
  workspace() { return this.json<Record<string, unknown>>('brain/workspace'); }
  dashboard() { return this.json<Record<string, unknown>>('brain/dashboard'); }
  queryData(query: DataRead) {
    return this.json<DataView>('data/query', 'POST', { schema_version: 1, query }, newKey('data-query'));
  }
  commandData(command: DataCommand, password: string) {
    return this.json<DataReceipt>('data/command', 'POST', { command, password }, command.command);
  }
  workbench(section: WorkbenchSection, offset = 0, snapshot: string | null = null, scope: string | null = null) {
    return this.json<WorkbenchSnapshot>('brain/workbench/query', 'POST', { schema_version: 1, section, scope, offset, limit: 50, snapshot }, newKey('view'));
  }
  reviewWorkbench(command: WorkbenchReview) {
    return this.json<WorkbenchReceipt>('brain/workbench/review', 'POST', command, command.command);
  }
  async memoryMaterial(item: WorkbenchItem, snapshot: string, scope: string | null = null) {
    const generation = this.generation;
    const value = await this.json<unknown>('brain/workbench/material', 'POST', {
      schema_version: 1, node: item.id, run: item.run, event: item.event, scope, snapshot,
    }, newKey('material-view'));
    const material = await parseMemoryMaterial(value, item);
    this.current(generation);
    return material;
  }
  resolveWorkbenchProposal(command: WorkbenchProposal) {
    return this.json<WorkbenchReceipt>('brain/workbench/proposal', 'POST', command, command.command);
  }
  async pendingWorkbenchProposals(): Promise<PendingWorkbenchProposals> {
    const value = await this.json<unknown>('workbench/proposals/pending');
    if (!isRecord(value) || value.schema_version !== 1 || !Array.isArray(value.commands)
        || value.commands.length > 256 || !value.commands.every(isWorkbenchProposal)
        || new Set(value.commands.map((command) => command.command)).size !== value.commands.length
        || new Set(value.commands.map((command) => JSON.stringify([command.run, command.proposal]))).size !== value.commands.length) {
      throw new ApiError(502, 'InvalidRecoveryState', '未决建议处置记录无法验证，已停止操作。');
    }
    return { schema_version: 1, commands: value.commands as WorkbenchProposal[] };
  }
  projects() { return this.json<OrchestrationLedger>('brain/projects'); }
  projectStatus() { return this.json<WorkspaceControlSnapshot>('brain/projects/status'); }
  projectSupervision(command: ProjectSupervisionCommand) {
    return this.json<WorkbenchReceipt>('brain/projects/supervision', 'POST', command, command.command);
  }
  project(command: ProjectCommand, key = newKey('project')) {
    return this.json<Record<string, unknown>>('brain/projects', 'POST', { schema_version: 1, request: command }, key);
  }
  charter(workspace: string) { return this.json<WorkspaceCharterRecord | null>(`brain/workspaces/${id(workspace)}/charter`); }
  prepareCharter(workspace: string, record: WorkspaceCharterRecord, expectedVersion: number, key = newKey('charter-prepare')) {
    return this.json<WorkspaceCharterChangedPayload>(`brain/workspaces/${id(workspace)}/charter`, 'POST', {
      schema_version: 1, command: key, record, expected_version: expectedVersion,
    }, key);
  }
  updateCharter(workspace: string, change: WorkspaceCharterChangedPayload, key = newKey('charter')) {
    return this.json<Record<string, unknown>>(`brain/workspaces/${id(workspace)}/charter`, 'PUT', { schema_version: 1, command: key, change }, key);
  }
  jobs() { return this.json<ScheduledJob[]>('brain/jobs'); }
  schedule(command: ScheduleCommand, key = newKey('schedule')) {
    return this.json<Record<string, unknown>>('brain/jobs', 'POST', command, key);
  }
  async cancelJob(intention: string, key = newKey('cancel-job')) {
    await this.request(`brain/jobs/${id(intention)}/cancel`, 'POST', undefined, key);
  }
  async registerLearningGoal(body: unknown, key = newKey('learning-goal')) {
    return this.json<Record<string, unknown>>('brain/learning/goals', 'POST', body, key);
  }
  async answerLearning(body: unknown, key = newKey('learning-answer')) {
    return this.json<Record<string, unknown>>('brain/learning/answer', 'POST', body, key);
  }
  async resolveLearning(body: unknown, key = newKey('learning-resolve')) {
    return this.json<Record<string, unknown>>('brain/learning/resolve', 'POST', body, key);
  }
}
