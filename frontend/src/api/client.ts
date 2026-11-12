import { parseChannelSettings, parseChannelResponse } from '../channel-settings.ts';
import type { ChannelSettingsPatch } from '../channel-settings.ts';
import { isRecord, versioned } from '../types.ts';
import { toolArtifactCatalog, readToolArtifact } from '../tool-artifacts.ts';
import type { ToolArtifactEntry } from '../tool-artifacts.ts';
import { parseDesktopCatalog, parseDesktopConsent } from '../desktop.ts';
import type { DesktopConsentRequest } from '../desktop.ts';
import { parseBootstrapCatalog, parseBootstrapCandidate } from '../bootstrap.ts';
import type { BootstrapCandidate } from '../bootstrap.ts';
import type { ArtifactCatalogView } from '../types.ts';
import type { ProjectSupervisionCommand, WorkspaceControlSnapshot } from '../types.ts';
import type { ArtifactUploadReceipt, ControlProfile, DataCandidate, HostStatus, OrchestrationLedger, PendingApproval, ProductModelProfilePatch, ProductNetworkConfig, ProductNetworkConfigPatch, ProductProxyConfigPatch, ProjectCommand, RunCommand, RunSummary, RuntimeView, ScheduleCommand, ScheduledJob, SessionResponse, WorkbenchSessionList, WorkbenchSessionRecord, WorkbenchSessionMutation, WorkspaceCharterChangedPayload, WorkspaceCharterRecord } from '../types.ts';
import type { ProductBrainPreferences, ProductCapabilitySettings, ProductConfigValidation, ProductCoreActivation, ProductCoreActivationResult, ProductOperation, RunPendingView } from '../types.ts';
import { isWorkbenchProposal } from '../workbench.ts';
import type { PendingWorkbenchProposals, WorkbenchProposal, WorkbenchReceipt, WorkbenchReview, WorkbenchSection, WorkbenchSnapshot } from '../workbench.ts';
import type { DataCommand, DataRead, DataReceipt, DataView } from '../data.ts';
import type { MaintenanceCommand, MaintenanceSnapshot } from '../maintenance.ts';
import { parseRuntimeDoctor } from '../runtime-diagnostics.ts';
import { parseMaterialLifecycleTarget, parseMemoryMaterial } from '../memory-material.ts';
import type { WorkbenchItem } from '../workbench.ts';
import { parseCorrectionReceipt, parseCorrectionsView } from '../correction.ts';
import type { CognitionCorrection } from '../correction.ts';
import { parseProductSettings } from '../settings-schema.ts';
import { parseComputerSettings, parseComputerSettingsResponse } from '../computer-settings.ts';
import type { ComputerSettingsPatch } from '../computer-settings.ts';
import { parseOfflineBackupVerification } from '../offline-backup.ts';
import { parseReleaseRecoveryCandidate, parseReleaseRecoveries, parseReleaseRecovery } from '../release-recovery.ts';
import type { ReleaseRecoveryCommand, ReleaseRecoveryVerification } from '../release-recovery.ts';
import { parseLegacyReview, parseLegacyRecoveries, parseLegacyRecovery, verifyLegacyRecoveryDigest } from '../legacy-release-recovery.ts';
import type { LegacyRecoveryCommand, LegacyRecoveryVerification } from '../legacy-release-recovery.ts';
import { parsePreparedCharter } from '../workspace-charter.ts';
import { parseMcpSettings, parseMcpSettingsResponse, parseMcpPrograms } from '../mcp-settings.ts';
import type { McpSettingsPatch, McpProgramCommand, McpDiscoveryRequest } from '../mcp-settings.ts';
import { parsePluginSettings, parsePluginResponse } from '../plugin-settings.ts';
import type { PluginSettingsPatch } from '../plugin-settings.ts';

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
  async pluginSettings() { return parsePluginSettings(await this.json<unknown>('product/plugin/settings')); }
  pluginOperation(operation: string) { return this.json<unknown>(`product/plugin/settings/operations/${id(operation)}`); }
  async savePluginSettings(patch: PluginSettingsPatch, password: string) {
    const {owner: _owner,...body} = patch;
    return parsePluginResponse(await this.json<unknown>('product/plugin/settings','POST',{...body,password},patch.operation_id),patch);
  }
  async channelSettings() { return parseChannelSettings(await this.json<unknown>('product/channel/settings')); }
  channelOperation(operation: string) { return this.json<unknown>(`product/channel/settings/operations/${id(operation)}`); }
  async saveChannelSettings(patch: ChannelSettingsPatch, password: string) {
    const {owner: _owner,...body} = patch;
    return parseChannelResponse(await this.json<unknown>('product/channel/settings','POST',{...body,password},patch.operation_id),patch);
  }
  private csrf: string | null = null;
  private generation = 0;
  private artifactReads = new AbortController();
  private readonly transport: typeof fetch;
  private readonly expired: () => void;
  constructor(transport = fetch, expired = () => {}) {
    // Browser fetch checks its receiver. Keep it detached from this client.
    this.transport = (...args) => transport(...args); this.expired = expired;
  }
  clear() { this.csrf = null; this.generation++; this.artifactReads.abort(); this.artifactReads = new AbortController(); }
  private current(generation: number) {
    if (generation !== this.generation) throw new ApiError(401, 'SessionChanged', '登录状态已变化，请重新读取当前页面。');
  }

  private async request(path: string, method = 'GET', body?: unknown, key?: string, options: { version?: 1 | 2; signal?: AbortSignal } = {}): Promise<Response> {
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
      response = await this.transport(`/api/v${options.version ?? 1}/${path}`, {
        method, headers, credentials: 'same-origin', cache: 'no-store',
        body: body === undefined ? undefined : JSON.stringify(body), signal: AbortSignal.any([AbortSignal.timeout(path === 'bootstrap/claim-v2' ? 140_000 : path.startsWith('bootstrap/') ? 50_000 : 28_000), ...(options.signal ? [options.signal] : [])]),
      });
    } catch {
      options.signal?.throwIfAborted();
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
  private async jsonValue(path: string, method = 'GET', body?: unknown, key?: string): Promise<unknown> {
    const generation = this.generation;
    const response = await this.request(path, method, body, key);
    if (response.status === 204) return undefined;
    const value: unknown = await response.json();
    this.current(generation);
    return value;
  }
  private async json<T>(path: string, method = 'GET', body?: unknown, key?: string, schema: 1 | 2 = 1): Promise<T> {
    const value = await this.jsonValue(path, method, body, key);
    if (value === undefined) return undefined as T;
    if (schema === 2) {
      if (!isRecord(value) || value.schema_version !== 2) throw new Error('设置协议不兼容，请更新页面或服务。');
    } else if (isRecord(value)) versioned(value);
    if (Array.isArray(value)) for (const item of value) if (isRecord(item)) versioned(item);
    return value as T;
  }
  async toolArtifacts(session: string, run: string, offset: number, snapshot: string | null, signal: AbortSignal) {
    const generation = this.generation;
    const bound = AbortSignal.any([signal, this.artifactReads.signal]);
    const result = await toolArtifactCatalog((path, signal) => this.request(path, 'GET', undefined, undefined, { version: 2, signal }), session, run, offset, snapshot, bound);
    this.current(generation); return result;
  }
  async toolArtifact(session: string, run: string, entry: ToolArtifactEntry, signal: AbortSignal) {
    const generation = this.generation;
    const bound = AbortSignal.any([signal, this.artifactReads.signal]);
    const result = await readToolArtifact((path, signal) => this.request(path, 'GET', undefined, undefined, { version: 2, signal }), session, run, entry, bound);
    this.current(generation); return result;
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
  async bootstrapCandidates() { return parseBootstrapCatalog(await this.json<unknown>('bootstrap/candidates', 'GET', undefined, undefined, 2)); }
  async selectLegacy(candidate: BootstrapCandidate, operation: string) {
    if (candidate.kind === 'new_brain_target' || candidate.status !== 'available') throw new Error('请选择已验证的原数据。');
    const selected = parseBootstrapCandidate(await this.json<unknown>('bootstrap/select-v2', 'POST', {
      schema_version: 2, candidate_ref: candidate.candidate_ref, expected_candidate_digest: candidate.candidate_digest,
    }, operation, 2));
    if (selected.candidate_ref !== candidate.candidate_ref || selected.candidate_digest !== candidate.candidate_digest
      || selected.verification_epoch !== candidate.verification_epoch || selected.kind !== candidate.kind
      || selected.status !== 'available' || selected.owner_ref !== candidate.owner_ref) throw new Error('数据选择已变化，请重新核实。');
    return selected;
  }
  async claimLegacy(candidate_ref: string, login_name: string, password: string, operation: string) {
    const view = await this.json<SessionResponse>('bootstrap/claim-v2', 'POST', { schema_version: 2, candidate_ref, login_name, password, operation_id: operation }, operation);
    this.csrf = view.csrf; return view;
  }
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
  async computerSettings() { return parseComputerSettings(await this.json<unknown>('desktop/settings')); }
  async mcpSettings() { return parseMcpSettings(await this.jsonValue('product/mcp/settings')); }
  async saveMcpSettings(patch: McpSettingsPatch, password: string) {
    const { owner: _owner, ...body } = patch;
    return parseMcpSettingsResponse(await this.jsonValue('product/mcp/settings','POST',{...body,password},patch.operation_id),patch);
  }
  mcpSettingsOperation(operation: string) { return this.jsonValue(`product/mcp/settings/operations/${id(operation)}`); }
  async mcpPrograms() { return parseMcpPrograms(await this.jsonValue('product/mcp/programs')); }
  registerMcpProgram(command: McpProgramCommand, password: string) {
    return this.jsonValue('product/mcp/programs/commands','POST',{schema_version:1,command,password},command.operation_id);
  }
  mcpProgramOperation(operation: string) { return this.jsonValue(`product/mcp/programs/operations/${id(operation)}`); }
  async discoverMcp(request: McpDiscoveryRequest, expectedRun: string) {
    const run = await this.jsonValue('brain/mcp/discovery','POST',request,request.command);
    if (run !== expectedRun) throw new Error('发现任务回执不匹配，请核实原任务。');
    return expectedRun;
  }
  async verifyMcpDiscovery(run: string, action: 'verify' | 'publish', command: string) {
    const result = await this.jsonValue(`brain/mcp/discovery/${id(run)}/${action}`,'POST',{schema_version:1,command},command);
    if (result !== run) throw new Error('原发现任务尚未核实。');
  }
  async saveComputerSettings(patch: ComputerSettingsPatch, password: string) {
    const { owner: _owner, ...body } = patch;
    return parseComputerSettingsResponse(await this.json<unknown>('desktop/settings', 'POST', { ...body, password }, patch.operation_id), patch);
  }
  computerSettingsOperation(operation: string) {
    return this.json<unknown>(`desktop/settings/operations/${id(operation)}`);
  }
  async productConfig() {
    return parseProductSettings(await this.json<unknown>('product/config?schema_version=2', 'GET', undefined, undefined, 2));
  }
  productStatus() { return this.json<HostStatus>('product/status'); }
  async saveProductConfig(expected_revision: number, models: ProductModelProfilePatch[], operation = newKey('config'), brain?: ProductBrainPreferences, capabilities?: ProductCapabilitySettings, network?: ProductNetworkConfig, proxy?: ProductProxyConfigPatch) {
    const networkPatch: ProductNetworkConfigPatch | undefined = network && {
      schema_version: network.schema_version,
      bind: network.bind,
      allowed_hosts: network.allowed_hosts,
      tls: network.tls && { schema_version: network.tls.schema_version, material_ref: network.tls.material_ref },
      firewall: network.firewall,
    };
    const response = await this.json<{ schema_version: 2; view: unknown; applied: boolean; operation_id: string | null; core_activation?: unknown }>(
      'product/config', 'POST', { schema_version: 2, operation_id: operation, expected_revision, models,
        ...(brain ? { brain } : {}), ...(capabilities ? { capabilities } : {}), ...(networkPatch ? { network: networkPatch } : {}), ...(proxy ? { proxy } : {}) }, operation, 2,
    );
    return { ...response, view: parseProductSettings(response.view) };
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
  async releaseRecoveryCandidate(original: string) {
    return parseReleaseRecoveryCandidate(await this.json<unknown>(`product/release-recovery/candidates/${id(original)}`), original);
  }
  async releaseRecoveries() { return parseReleaseRecoveries(await this.json<unknown>('product/release-recovery/operations')); }
  async releaseRecoveryOperation(operation: string) {
    return parseReleaseRecovery(await this.json<unknown>(`product/release-recovery/operations/${id(operation)}`), operation);
  }
  async recoverRelease(command: ReleaseRecoveryCommand, password: string) {
    const view = parseReleaseRecovery(await this.json<unknown>('product/release-recovery', 'POST', { ...command, password }, command.operation_id), command.operation_id);
    const { owner, ...bound } = view.request;
    if (owner !== command.candidate.owner || JSON.stringify(bound) !== JSON.stringify(command)) {
      // Field order is not authority; compare the exact typed fields below.
      const fields = Object.keys(command) as (keyof ReleaseRecoveryCommand)[];
      if (owner !== command.candidate.owner || fields.some((field) => JSON.stringify(bound[field]) !== JSON.stringify(command[field]))) throw new Error('恢复回执与原请求不符。');
    }
    return view;
  }
  async verifyReleaseRecovery(command: ReleaseRecoveryVerification, password: string) {
    const view = parseReleaseRecovery(await this.json<unknown>('product/release-recovery/verify', 'POST', { ...command, password }, command.operation_id), command.operation_id);
    if (view.request_digest !== command.request_digest) throw new Error('恢复回执与原裁决不符。');
    return view;
  }
  async legacyReleaseReview(original: string) {
    return parseLegacyReview(await this.json<unknown>(`product/legacy-release-recovery/review/${id(original)}`), original);
  }
  async legacyReleaseRecoveries() {
    const list = parseLegacyRecoveries(await this.json<unknown>('product/legacy-release-recovery/operations'));
    await Promise.all(list.operations.map(verifyLegacyRecoveryDigest));
    return list;
  }
  async legacyReleaseRecoveryOperation(operation: string) {
    return verifyLegacyRecoveryDigest(parseLegacyRecovery(await this.json<unknown>(`product/legacy-release-recovery/operations/${id(operation)}`), operation));
  }
  async recoverLegacyRelease(command: LegacyRecoveryCommand, password: string) {
    const view = parseLegacyRecovery(await this.json<unknown>('product/legacy-release-recovery', 'POST', { ...command, password }, command.operation_id), command.operation_id);
    if ((Object.keys(command) as (keyof LegacyRecoveryCommand)[]).some((key) => view.request[key] !== command[key])) throw new Error('历史恢复回执与原请求不符。');
    return verifyLegacyRecoveryDigest(view);
  }
  async verifyLegacyReleaseRecovery(command: LegacyRecoveryVerification, password: string) {
    const view = parseLegacyRecovery(await this.json<unknown>('product/legacy-release-recovery/verify', 'POST', { ...command, password }, command.operation_id), command.operation_id);
    if (view.request_digest !== command.request_digest) throw new Error('历史恢复回执与原决定不符。');
    return verifyLegacyRecoveryDigest(view);
  }
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
  async desktopSessions() { return parseDesktopCatalog(await this.jsonValue('desktop/sessions')); }
  async desktopConsent(pendingRef: string) { return parseDesktopConsent(await this.jsonValue(`desktop/consents/${id(pendingRef)}`),pendingRef); }
  async consentDesktop(request: DesktopConsentRequest,password: string) {
    return parseDesktopConsent(await this.jsonValue('desktop/consents','POST',{schema_version:1,request,password},request.operation_id),request.pending_ref,request.operation_id);
  }

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
  async verifyOfflineBackup(source: string, config_revision: number) {
    const value = await this.json<unknown>('product/data/verify', 'POST', {
      schema_version: 1, source, config_revision,
    }, newKey('backup-query'));
    return parseOfflineBackupVerification(value, source, config_revision);
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
  async correctCognition(command: CognitionCorrection) {
    return parseCorrectionReceipt(await this.json<unknown>('brain/workbench/correction', 'POST', command, command.command), command.command);
  }
  async corrections(command?: string) {
    return parseCorrectionsView(await this.json<unknown>('brain/workbench/corrections/query', 'POST', {
      schema_version: 1, command: command ?? null,
    }, newKey('correction-query')), command);
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
  async materialLifecycleTarget(item: WorkbenchItem, snapshot: string, scope: string | null = null) {
    const generation = this.generation;
    const value = await this.json<unknown>('brain/workbench/material-lifecycle', 'POST', {
      schema_version: 1, node: item.id, run: item.run, event: item.event, scope, snapshot,
    }, newKey('material-lifecycle-view'));
    const target = parseMaterialLifecycleTarget(value, item);
    this.current(generation);
    return target;
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
  async project(command: ProjectCommand, key = newKey('project')) {
    const value = await this.jsonValue('brain/projects', 'POST', { schema_version: 1, request: command }, key);
    if (isRecord(value) && 'schema_version' in value) versioned(value);
    if (command.operation === 'register' || command.operation === 'revise') {
      if (!isRecord(value) || typeof value.event !== 'string' || !value.event.trim()) throw new Error('项目操作回执不完整，请核实原命令。');
    } else if (command.operation === 'cancel') {
      if (!isRecord(value) || value.cancelled !== true) throw new Error('路线取消回执不完整，请核实原命令。');
    } else versioned(value);
    return value as Record<string, unknown>;
  }
  charter(workspace: string) { return this.json<WorkspaceCharterRecord | null>(`brain/workspaces/${id(workspace)}/charter`); }
  async prepareCharter(workspace: string, record: WorkspaceCharterRecord, expectedVersion: number, key = newKey('charter-prepare')) {
    const value = await this.jsonValue(`brain/workspaces/${id(workspace)}/charter`, 'POST', {
      schema_version: 1, command: key, record, expected_version: expectedVersion,
    }, key);
    return parsePreparedCharter(value, record, expectedVersion);
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
