import { isRecord } from './types.ts';

export interface ComputerPlacement {
  schema_version: 1; enabled: boolean; scope: string; surface_refs: string[];
}
export interface ComputerSettingsView {
  schema_version: 1; owner: string | null; active_revision: number; pending_restart: boolean;
  configured: boolean; computer: ComputerPlacement; available: boolean;
  available_scope: string | null; available_surfaces: string[];
}
export interface ComputerSettingsPatch {
  schema_version: 1; operation_id: string; expected_revision: number;
  owner: string; computer: ComputerPlacement;
}
const invalid = () => new Error('桌面设置尚未核实，请重新读取。');
const id = (v: unknown): v is string => typeof v === 'string' && /^[A-Za-z0-9:_.-]{1,128}$/.test(v) && v !== '.' && v !== '..';
function closed(v: unknown, keys: string[]): asserts v is Record<string, unknown> {
  if (!isRecord(v) || v.schema_version !== 1 || Object.keys(v).length !== keys.length
      || keys.some((key) => !Object.hasOwn(v,key))) throw invalid();
}
function refs(v: unknown): v is string[] {
  return Array.isArray(v) && v.length <= 64 && v.every(id) && new Set(v).size === v.length;
}
export function parseComputerSettings(v: unknown): ComputerSettingsView {
  closed(v,['schema_version','owner','active_revision','pending_restart','configured','computer','available','available_scope','available_surfaces']);
  if ((v.owner !== null && !id(v.owner)) || !Number.isSafeInteger(v.active_revision)
      || Number(v.active_revision) < 0 || Number(v.active_revision) > 1_000_000_000
      || typeof v.pending_restart !== 'boolean' || typeof v.configured !== 'boolean'
      || typeof v.available !== 'boolean' || !refs(v.available_surfaces)
      || (v.available ? !id(v.available_scope) || v.available_surfaces.length === 0
        : v.available_scope !== null || v.available_surfaces.length !== 0)) throw invalid();
  closed(v.computer,['schema_version','enabled','scope','surface_refs']);
  if (typeof v.computer.enabled !== 'boolean' || !id(v.computer.scope) || !refs(v.computer.surface_refs)
      || (v.computer.enabled && v.computer.surface_refs.length === 0)) throw invalid();
  return v as unknown as ComputerSettingsView;
}
export function computerSettingsPatch(view: ComputerSettingsView, enabled: boolean, operation: string): ComputerSettingsPatch {
  parseComputerSettings(view);
  if (!id(operation) || !id(view.owner) || !view.configured || (enabled && !view.available)) throw invalid();
  return {
    schema_version: 1, operation_id: operation, expected_revision: view.active_revision, owner: view.owner,
    computer: { schema_version: 1, enabled, scope: enabled ? view.available_scope! : view.computer.scope,
      surface_refs: [...(enabled ? view.available_surfaces : view.computer.surface_refs)] },
  };
}
export async function computerSettingsDigest(patch: ComputerSettingsPatch): Promise<string> {
  // Match the frozen Rust struct order, regardless of caller object key order.
  const ordered = { schema_version: patch.schema_version, operation_id: patch.operation_id,
    expected_revision: patch.expected_revision, owner: patch.owner,
    computer: { schema_version: patch.computer.schema_version, enabled: patch.computer.enabled,
      scope: patch.computer.scope, surface_refs: patch.computer.surface_refs } };
  const data = new TextEncoder().encode(JSON.stringify(['forme.product.computer-settings.v1', ordered]));
  const digest = new Uint8Array(await crypto.subtle.digest('SHA-256',data));
  return `sha256:${Array.from(digest, (byte) => byte.toString(16).padStart(2,'0')).join('')}`;
}
export function parseComputerSettingsResponse(v: unknown, patch: ComputerSettingsPatch): ComputerSettingsView {
  closed(v,['schema_version','operation_id','applied','view']);
  if (v.operation_id !== patch.operation_id || typeof v.applied !== 'boolean') throw invalid();
  const view = parseComputerSettings(v.view);
  if (view.owner !== patch.owner || view.active_revision <= patch.expected_revision) throw invalid();
  return view;
}
export function verifyComputerSettingsOperation(v: unknown, patch: ComputerSettingsPatch, expectedDigest: string): void {
  closed(v,['schema_version','owner','operation_id','request_digest','committed']);
  if (v.owner !== patch.owner || v.operation_id !== patch.operation_id
      || v.request_digest !== expectedDigest || v.committed !== true) throw invalid();
}
