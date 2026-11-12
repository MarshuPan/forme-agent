import { isRecord } from './types.ts';
import { mcpDigest, mcpId } from './mcp-settings.ts';

export interface PluginConfig {
  schema_version: 1; plugin_ref: string; source_ref: string; expected_source_digest: string;
  scope: string; enabled: boolean; trust: 'untrusted' | 'owner_input';
}
export interface PluginSettingsView {
  schema_version: 1; owner: string | null; active_revision: number; pending_restart: boolean;
  configured: boolean; plugins: PluginConfig[]; sources: PluginConfig[]; sources_available: boolean;
}
export interface PluginSettingsPatch {
  schema_version: 1; operation_id: string; expected_revision: number; owner: string; plugins: PluginConfig[];
}
const invalid = () => new Error('插件设置或原操作回执尚未核实，请重新读取。');
function closed(v: unknown, keys: string[]): asserts v is Record<string,unknown> {
  if (!isRecord(v) || v.schema_version !== 1 || Object.keys(v).length !== keys.length
    || keys.some((key) => !Object.hasOwn(v,key))) throw invalid();
}
function entries(v: unknown): PluginConfig[] {
  if (!Array.isArray(v) || v.length > 64) throw invalid();
  for (const p of v) {
    closed(p,['schema_version','plugin_ref','source_ref','expected_source_digest','scope','enabled','trust']);
    if (![p.plugin_ref,p.source_ref,p.scope].every(mcpId) || typeof p.expected_source_digest !== 'string'
      || !/^sha256:[a-f0-9]{64}$/.test(p.expected_source_digest) || typeof p.enabled !== 'boolean'
      || !['untrusted','owner_input'].includes(String(p.trust))) throw invalid();
  }
  if (new Set(v.map((p) => p.plugin_ref)).size !== v.length) throw invalid();
  return v as PluginConfig[];
}
export function parsePluginSettings(v: unknown): PluginSettingsView {
  closed(v,['schema_version','owner','active_revision','pending_restart','configured','plugins','sources','sources_available']);
  if ((v.owner !== null && !mcpId(v.owner)) || !Number.isSafeInteger(v.active_revision)
    || Number(v.active_revision) < 0 || Number(v.active_revision) > 1_000_000_000
    || ![v.pending_restart,v.configured,v.sources_available].every((b) => typeof b === 'boolean')) throw invalid();
  entries(v.plugins);
  if (entries(v.sources).some((s) => s.enabled || s.trust !== 'untrusted')) throw invalid();
  return v as unknown as PluginSettingsView;
}
export function pluginSettingsPatch(v: PluginSettingsView, plugins: PluginConfig[], operation: string): PluginSettingsPatch {
  parsePluginSettings(v); entries(plugins);
  if (!mcpId(v.owner) || !mcpId(operation) || !v.configured || plugins.some((p) => p.enabled && (p.trust !== 'owner_input'
    || !v.sources_available || !v.sources.some((s) => s.source_ref === p.source_ref && s.plugin_ref === p.plugin_ref
      && s.expected_source_digest === p.expected_source_digest && s.scope === p.scope)))) throw invalid();
  return {schema_version:1,operation_id:operation,expected_revision:v.active_revision,owner:v.owner,plugins:structuredClone(plugins)};
}
export function pluginSettingsDigest(p: PluginSettingsPatch) {
  return mcpDigest(['forme.product.plugin-settings.v1',{schema_version:p.schema_version,operation_id:p.operation_id,
    expected_revision:p.expected_revision,owner:p.owner,plugins:p.plugins.map((s) => ({schema_version:s.schema_version,
      plugin_ref:s.plugin_ref,source_ref:s.source_ref,expected_source_digest:s.expected_source_digest,scope:s.scope,enabled:s.enabled,trust:s.trust}))}]);
}
export function verifyPluginOperation(v: unknown, p: PluginSettingsPatch, digest: string) {
  closed(v,['schema_version','owner','operation_id','request_digest','committed']);
  if (v.owner !== p.owner || v.operation_id !== p.operation_id || v.request_digest !== digest || v.committed !== true) throw invalid();
}
export function parsePluginResponse(v: unknown, p: PluginSettingsPatch): PluginSettingsView {
  closed(v,['schema_version','operation_id','applied','view']);
  const view = parsePluginSettings(v.view);
  if (v.operation_id !== p.operation_id || typeof v.applied !== 'boolean' || view.owner !== p.owner || view.active_revision <= p.expected_revision) throw invalid();
  return view;
}
