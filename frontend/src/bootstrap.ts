import { isRecord } from './types.ts';

export interface BootstrapCandidate {
  schema_version: 2;
  candidate_ref: string;
  candidate_digest: string;
  verification_epoch: number;
  kind: 'new_brain_target' | 'existing_brain' | 'verified_backup';
  status: 'available' | 'rejected' | 'unverifiable' | 'conflict';
  owner_ref: string | null;
}
export interface BootstrapCatalog {
  operation: string;
  candidates: BootstrapCandidate[];
  selected: string | null;
}
const digest = (value: unknown) => typeof value === 'string' && /^sha256:[0-9a-f]{64}$/.test(value);
const reference = (value: unknown) => typeof value === 'string' && /^[A-Za-z0-9._:-]{1,128}$/.test(value) && value !== '.' && value !== '..';
export function parseBootstrapCandidate(value: unknown): BootstrapCandidate {
  if (!isRecord(value) || value.schema_version !== 2 || !reference(value.candidate_ref)
    || !digest(value.candidate_digest) || !Number.isSafeInteger(value.verification_epoch) || Number(value.verification_epoch) <= 0
    || !['new_brain_target', 'existing_brain', 'verified_backup'].includes(String(value.kind))
    || !['available', 'rejected', 'unverifiable', 'conflict'].includes(String(value.status))
    || !(value.owner_ref === null || typeof value.owner_ref === 'string')) throw new Error('初始化候选协议不兼容。');
  if (value.status === 'available' && value.kind !== 'new_brain_target'
    && (!reference(value.owner_ref) || !(value.owner_ref as string).startsWith('owner:')
      || value.owner_source !== 'verified_from_candidate' || value.protection_state !== 'protected'
      || !digest(value.owner_binding_digest) || !digest(value.store_identity_digest) || !digest(value.schema_digest))) {
    throw new Error('已有数据尚未完成身份验证。');
  }
  if (value.kind === 'new_brain_target' && (value.owner_ref !== null || value.owner_source !== 'generated_for_new_brain'
    || value.owner_binding_digest !== null || value.store_identity_digest !== null || value.schema_digest !== null)) throw new Error('新主脑候选身份不一致。');
  if (value.status === 'available' && (value.protection_state !== 'protected' || value.issue_ref !== null)) throw new Error('数据候选尚未通过验证。');
  return value as unknown as BootstrapCandidate;
}
export function parseBootstrapCatalog(value: unknown): BootstrapCatalog {
  if (!isRecord(value) || value.schema_version !== 2 || !reference(value.operation_id)) throw new Error('初始化候选协议不兼容。');
  if (value.selected !== null) {
    if (value.catalog !== null) throw new Error('初始化选择状态不一致。');
    const selected = parseBootstrapCandidate(value.selected);
    return { operation: value.operation_id as string, candidates: [selected], selected: selected.candidate_ref };
  }
  const catalog = value.catalog;
  if (!isRecord(catalog) || catalog.schema_version !== 2 || !Array.isArray(catalog.candidates)
    || catalog.candidates.length < 1 || catalog.candidates.length > 9 || !digest(catalog.catalog_digest)) throw new Error('初始化候选列表无效。');
  const candidates = catalog.candidates.map(parseBootstrapCandidate);
  if (candidates.some(candidate => candidate.verification_epoch !== catalog.verification_epoch)
    || new Set(candidates.map(candidate => candidate.candidate_ref)).size !== candidates.length) throw new Error('初始化候选观察不一致。');
  return { operation: value.operation_id as string, candidates, selected: null };
}
export function candidateLabel(candidate: BootstrapCandidate): string {
  const kind = { new_brain_target: '创建全新主脑', existing_brain: '接管已有主脑', verified_backup: '从已验证备份恢复' }[candidate.kind];
  return candidate.owner_ref ? `${kind} · ${candidate.owner_ref}` : kind;
}
