import { isRecord } from './types.ts';
import type { PendingApproval } from './types.ts';

export interface DesktopChoice {
  schema_version: 1; session_ref: string; platform: 'windows' | 'macos' | 'linux'; label: string;
  available: boolean; supported_capabilities: string[];
}
export interface DesktopCatalog {
  schema_version: 1; installation_ref: string; owner_ref: string; authority_epoch: number;
  catalog_revision: number; catalog_digest: string; entries: DesktopChoice[]; observed_at: number; expires_at: number;
}
export interface DesktopConsentView {
  schema_version: 1; operation_id: string; revision: number; pending_ref: string; consent_ref: string;
  status: 'prepared' | 'committed' | 'revoked' | 'expired' | 'unknown'; expires_at: number; reason_ref: string | null;
}
export interface DesktopConsentRequest {
  schema_version: 1; operation_id: string; expected_revision: number; pending_ref: string;
  catalog_revision: number; catalog_digest: string; session_ref: string; expires_at: number;
}
const invalid = () => new Error('桌面授权信息无法核实，请重新读取。');
const identifier = (v: unknown): v is string => typeof v === 'string' && /^[a-zA-Z0-9_.:-]{1,128}$/.test(v);
const positive = (v: unknown): v is number => Number.isSafeInteger(v) && Number(v) > 0;
const digest = (v: unknown): v is string => typeof v === 'string' && /^sha256:[a-f0-9]{64}$/.test(v);
function closed(v: unknown, keys: string[]): asserts v is Record<string, unknown> {
  if (!isRecord(v) || v.schema_version !== 1 || Object.keys(v).length !== keys.length
      || keys.some((key) => !Object.hasOwn(v, key))) throw invalid();
}
export function parseDesktopCatalog(v: unknown, now = Date.now()): DesktopCatalog {
  closed(v, ['schema_version','installation_ref','owner_ref','authority_epoch','catalog_revision','catalog_digest','entries','observed_at','expires_at']);
  if (![v.installation_ref,v.owner_ref].every(identifier) || ![v.authority_epoch,v.catalog_revision,v.observed_at,v.expires_at].every(positive)
      || !digest(v.catalog_digest) || !Array.isArray(v.entries) || v.entries.length > 64
      || Number(v.observed_at) > now || Number(v.expires_at) <= now || Number(v.expires_at) - Number(v.observed_at) > 30_000) throw invalid();
  const refs = new Set<string>();
  for (const item of v.entries) {
    closed(item, ['schema_version','session_ref','platform','label','available','supported_capabilities']);
    if (!identifier(item.session_ref) || refs.has(item.session_ref) || !['windows','macos','linux'].includes(String(item.platform))
        || typeof item.label !== 'string' || !item.label.trim() || new TextEncoder().encode(item.label).length > 128
        || /[\u0000-\u001f\u007f]/.test(item.label) || typeof item.available !== 'boolean' || !Array.isArray(item.supported_capabilities)
        || item.supported_capabilities.some((cap) => !['pointer_move','pointer_click','keyboard_type','keyboard_key','scroll','screenshot','pty_input'].includes(String(cap)))
        || new Set(item.supported_capabilities).size !== item.supported_capabilities.length || (item.available && !item.supported_capabilities.length)) throw invalid();
    refs.add(item.session_ref);
  }
  return v as unknown as DesktopCatalog;
}
export function parseDesktopConsent(v: unknown, pendingRef: string, operation?: string): DesktopConsentView {
  closed(v, ['schema_version','operation_id','revision','pending_ref','consent_ref','status','expires_at','reason_ref']);
  if (![v.operation_id,v.pending_ref,v.consent_ref].every(identifier) || ![v.revision,v.expires_at].every(positive)
      || v.pending_ref !== pendingRef || (operation !== undefined && v.operation_id !== operation)
      || !['prepared','committed','revoked','expired','unknown'].includes(String(v.status))
      || (v.reason_ref !== null && !identifier(v.reason_ref))) throw invalid();
  return v as unknown as DesktopConsentView;
}
export function isDesktopApproval(item: PendingApproval): boolean {
  return item.affected_resources.some((resource) => resource.startsWith('computer-driver:'));
}
export async function desktopPendingRef(item: Pick<PendingApproval, 'approval_id' | 'plan_digest'>): Promise<string> {
  if (!identifier(item.approval_id) || !digest(item.plan_digest)) throw invalid();
  const bytes = new TextEncoder().encode(JSON.stringify(['forme.product.desktop.pending.v1','reference',item.approval_id,item.plan_digest]));
  const hashed = new Uint8Array(await crypto.subtle.digest('SHA-256',bytes));
  return `desktop-pending:${Array.from(hashed, (v) => v.toString(16).padStart(2,'0')).join('')}`;
}
export function consentReady(view: DesktopConsentView | null, now = Date.now()): boolean {
  return view?.status === 'committed' && view.expires_at > now;
}
export async function desktopApprovalOperation(item: Pick<PendingApproval, 'approval_id' | 'plan_digest'>): Promise<string> {
  return (await desktopPendingRef(item)).replace('desktop-pending:', 'desktop-approval:');
}
