import { isRecord } from './types.ts';

export interface McpServer {
  schema_version: 1; server_ref: string; program_ref: string; scope: string;
  allowlist: { schema_version: 1; tools: string[]; resources: string[] };
  enabled: boolean; trust: 'untrusted' | 'owner_input'; timeout: number; output_budget: number;
}
export interface McpSettingsView {
  schema_version: 1; owner: string | null; active_revision: number; pending_restart: boolean;
  configured: boolean; mcp_servers: McpServer[]; available: boolean;
}
export interface McpSettingsPatch {
  schema_version: 1; operation_id: string; expected_revision: number; owner: string; mcp_servers: McpServer[];
}
export interface McpProgram {
  schema_version: 1; program_ref: string; registration_digest: string; source_ref: string;
  scope: string; sandbox_profile_ref: string; revoked: boolean;
}
export interface McpChoice { schema_version: 1; source_ref: string; source_digest: string; scope: string; sandbox_profile_ref: string }
export interface McpProgramCatalog {
  schema_version: 1; installation_ref: string; owner_ref: string; authority_epoch: number; revision: number;
  choices: McpChoice[]; programs: McpProgram[];
}
export interface McpProgramCommand {
  schema_version: 1; installation_ref: string; owner_ref: string; authority_epoch: number;
  operation_id: string; expected_revision: number; requested_deadline: number;
  action: { kind: 'register'; source_ref: string; source_digest: string } | { kind: 'revoke'; program_ref: string; registration_digest: string };
}
export interface McpDiscoveryRequest {
  schema_version: 1; session: string; command: string;
  spec: { schema_version: 1; server: string; program_ref: string; registration_digest: string; settings_digest: string; timeout: number; output_budget_bytes: number };
}
const invalid = () => new Error('MCP 配置或操作回执尚未核实，请重新读取。');
export const mcpId = (v: unknown): v is string => typeof v === 'string' && /^[A-Za-z0-9:_.-]{1,128}$/.test(v) && v !== '.' && v !== '..';
const digest = (v: unknown): v is string => typeof v === 'string' && /^sha256:[a-f0-9]{64}$/.test(v);
const number = (v: unknown, min = 0, max = Number.MAX_SAFE_INTEGER): v is number => Number.isSafeInteger(v) && Number(v) >= min && Number(v) <= max;
function closed(v: unknown, keys: string[]): asserts v is Record<string, unknown> {
  if (!isRecord(v) || v.schema_version !== 1 || Object.keys(v).length !== keys.length
    || keys.some((key) => !Object.hasOwn(v, key))) throw invalid();
}
function names(v: unknown): v is string[] {
  return Array.isArray(v) && v.length <= 128 && v.every((s) => typeof s === 'string' && s.trim()
    && new TextEncoder().encode(s).length <= 2048 && !/[\p{Cc}*]/u.test(s)) && new Set(v).size === v.length;
}
export function parseMcpServer(v: unknown): McpServer {
  closed(v, ['schema_version','server_ref','program_ref','scope','allowlist','enabled','trust','timeout','output_budget']);
  closed(v.allowlist, ['schema_version','tools','resources']);
  if (![v.server_ref,v.program_ref,v.scope].every(mcpId) || typeof v.enabled !== 'boolean'
    || !['untrusted','owner_input'].includes(String(v.trust)) || !number(v.timeout,1,60_000)
    || !number(v.output_budget,1,1_048_576) || !names(v.allowlist.tools) || !names(v.allowlist.resources)) throw invalid();
  return v as unknown as McpServer;
}
function servers(v: unknown): McpServer[] {
  if (!Array.isArray(v) || v.length > 64) throw invalid();
  const parsed = v.map(parseMcpServer);
  if (new Set(parsed.map((s) => s.server_ref)).size !== parsed.length
    || new TextEncoder().encode(JSON.stringify(parsed)).length > 65_536) throw invalid();
  return parsed;
}
export function parseMcpSettings(v: unknown): McpSettingsView {
  closed(v,['schema_version','owner','active_revision','pending_restart','configured','mcp_servers','available']);
  if ((v.owner !== null && !mcpId(v.owner)) || !number(v.active_revision,0,1_000_000_000)
    || ![v.pending_restart,v.configured,v.available].every((b) => typeof b === 'boolean')) throw invalid();
  servers(v.mcp_servers);
  return v as unknown as McpSettingsView;
}
export function mcpSettingsPatch(view: McpSettingsView, entries: McpServer[], operation: string): McpSettingsPatch {
  parseMcpSettings(view); servers(entries);
  if (!mcpId(operation) || !mcpId(view.owner) || !view.configured
    || entries.some((s) => s.enabled && (!view.available || s.trust !== 'owner_input' || s.allowlist.tools.length + s.allowlist.resources.length === 0))) throw invalid();
  return { schema_version: 1, operation_id: operation, expected_revision: view.active_revision, owner: view.owner,
    mcp_servers: structuredClone(entries) };
}
function orderedServer(s: McpServer): McpServer {
  parseMcpServer(s);
  return { schema_version: 1, server_ref: s.server_ref, program_ref: s.program_ref, scope: s.scope,
    allowlist: { schema_version: 1, tools: [...s.allowlist.tools], resources: [...s.allowlist.resources] },
    enabled: s.enabled, trust: s.trust, timeout: s.timeout, output_budget: s.output_budget };
}
export async function mcpDigest(value: unknown): Promise<string> {
  const bytes = new TextEncoder().encode(JSON.stringify(value));
  const hash = new Uint8Array(await crypto.subtle.digest('SHA-256',bytes));
  return `sha256:${Array.from(hash,(b) => b.toString(16).padStart(2,'0')).join('')}`;
}
export function mcpSettingsDigest(p: McpSettingsPatch) {
  return mcpDigest(['forme.product.mcp-settings.v1', { schema_version:p.schema_version,operation_id:p.operation_id,
    expected_revision:p.expected_revision,owner:p.owner,mcp_servers:p.mcp_servers.map(orderedServer) }]);
}
export function parseMcpSettingsResponse(v: unknown, p: McpSettingsPatch): McpSettingsView {
  closed(v,['schema_version','operation_id','applied','view']);
  const view = parseMcpSettings(v.view);
  if (v.operation_id !== p.operation_id || typeof v.applied !== 'boolean' || view.owner !== p.owner || view.active_revision <= p.expected_revision) throw invalid();
  return view;
}
export function verifyMcpSettingsOperation(v: unknown, p: McpSettingsPatch, expected: string) {
  closed(v,['schema_version','owner','operation_id','request_digest','committed']);
  if (v.owner !== p.owner || v.operation_id !== p.operation_id || v.request_digest !== expected || v.committed !== true) throw invalid();
}
function placement(v: Record<string,unknown>) {
  if (!mcpId(v.source_ref) || !mcpId(v.scope) || v.sandbox_profile_ref !== 'windows-appcontainer-mcp-stdio-v1') throw invalid();
}
function program(v: unknown): McpProgram {
  closed(v,['schema_version','program_ref','registration_digest','source_ref','scope','sandbox_profile_ref','revoked']); placement(v);
  if (!digest(v.registration_digest) || v.program_ref !== `mcp-program:${v.registration_digest.slice(7)}` || typeof v.revoked !== 'boolean') throw invalid();
  return v as unknown as McpProgram;
}
export function parseMcpPrograms(v: unknown): McpProgramCatalog {
  closed(v,['schema_version','installation_ref','owner_ref','authority_epoch','revision','choices','programs']);
  if (!mcpId(v.installation_ref) || !mcpId(v.owner_ref) || !number(v.authority_epoch,1) || !number(v.revision,1)
    || !Array.isArray(v.choices) || v.choices.length > 64 || !Array.isArray(v.programs) || v.programs.length > 64) throw invalid();
  for (const c of v.choices) {
    closed(c,['schema_version','source_ref','source_digest','scope','sandbox_profile_ref']); placement(c);
    if (!digest(c.source_digest)) throw invalid();
  }
  v.programs.forEach(program);
  if (new Set(v.choices.map((c) => c.source_ref)).size !== v.choices.length
    || new Set(v.programs.map((p) => p.program_ref)).size !== v.programs.length) throw invalid();
  return v as unknown as McpProgramCatalog;
}
export function mcpProgramCommand(c: McpProgramCatalog, action: McpProgramCommand['action'], operation: string, now: number): McpProgramCommand {
  parseMcpPrograms(c);
  if (!mcpId(operation) || !number(now,1)) throw invalid();
  if (action.kind === 'register' ? !c.choices.some((v) => v.source_ref === action.source_ref && v.source_digest === action.source_digest)
    : !c.programs.some((v) => !v.revoked && v.program_ref === action.program_ref && v.registration_digest === action.registration_digest)) throw invalid();
  return { schema_version:1, installation_ref:c.installation_ref, owner_ref:c.owner_ref,authority_epoch:c.authority_epoch,
    operation_id:operation,expected_revision:c.revision,requested_deadline:now + 120_000,action:structuredClone(action) };
}
export function mcpProgramDigest(c: McpProgramCommand) {
  const action = c.action.kind === 'register' ? {kind:'register',source_ref:c.action.source_ref,source_digest:c.action.source_digest}
    : {kind:'revoke',program_ref:c.action.program_ref,registration_digest:c.action.registration_digest};
  return mcpDigest(['forme.managed-mcp.program.command.v1',{schema_version:c.schema_version,installation_ref:c.installation_ref,
    owner_ref:c.owner_ref,authority_epoch:c.authority_epoch,operation_id:c.operation_id,expected_revision:c.expected_revision,
    requested_deadline:c.requested_deadline,action}]);
}
export function verifyMcpProgramOperation(v: unknown, command: McpProgramCommand, expected: string) {
  closed(v,['schema_version','operation_id','command_digest','revision','program']);
  if (v.operation_id !== command.operation_id || v.command_digest !== expected || !number(v.revision,command.expected_revision + 1)) throw invalid();
  const result = program(v.program);
  if (command.action.kind === 'register' ? result.revoked || result.source_ref !== command.action.source_ref
    : !result.revoked || result.program_ref !== command.action.program_ref || result.registration_digest !== command.action.registration_digest) throw invalid();
  return result;
}
export async function mcpDiscoveryRequest(owner: string, server: McpServer, registered: McpProgram, command: string): Promise<{ request: McpDiscoveryRequest; run: string }> {
  parseMcpServer(server); program(registered);
  if (!mcpId(owner) || !mcpId(command) || !server.enabled || server.trust !== 'owner_input' || registered.revoked
    || server.program_ref !== registered.program_ref || server.scope !== registered.scope) throw invalid();
  const session = `session:mcp:${(await mcpDigest(server.server_ref)).slice(7)}`;
  const request: McpDiscoveryRequest = { schema_version:1,session,command,spec: {schema_version:1,server:server.server_ref,
    program_ref:server.program_ref,registration_digest:registered.registration_digest,
    settings_digest:await mcpDigest(['forme.managed-mcp.settings.v1',orderedServer(server)]),
    timeout:server.timeout,output_budget_bytes:server.output_budget} };
  return { request, run:`run:owner-mcp:${await mcpDigest([owner,session,command])}` };
}
