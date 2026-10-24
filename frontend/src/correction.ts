import { isRecord } from './types.ts';
import type { WorkbenchItem } from './workbench.ts';

export interface CognitionCorrection {
  schema_version: 1; command: string; run: string; kind: 'Candidate' | 'Stable';
  id: string; event: string; snapshot: string; statement: string;
}
export interface CorrectionReceipt {
  schema_version: 1; command: string; binding: string; status: 'Confirmed'; event: string; candidate: string;
}
export interface CorrectionsView { schema_version: 1; receipts: CorrectionReceipt[]; truncated: boolean }

const validId = (value: unknown): value is string => typeof value === 'string' && value.trim().length > 0
  && new TextEncoder().encode(value).length <= 256 && !/\p{Cc}/u.test(value);
const digest = (value: unknown): value is string => typeof value === 'string' && /^sha256:[0-9a-f]{64}$/.test(value);

export function canCorrect(item: WorkbenchItem): boolean {
  return !!item.scope && !!item.tier && !['fixed', 'constitutional'].includes(item.tier.toLowerCase())
    && ((item.kind === 'Candidate' && ['Candidate', 'Promoted'].includes(item.state))
      || (item.kind === 'Stable' && item.state === 'Active'));
}

export function correctionCommand(item: WorkbenchItem, snapshot: string, statement: string, command: string): CognitionCorrection {
  if (!canCorrect(item) || !digest(snapshot) || ![command, item.run, item.id, item.event].every(validId)
      || !statement.trim() || new TextEncoder().encode(statement).length > 8192 || /[\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f-\u009f]/u.test(statement)) {
    throw new Error('请核对当前认知来源，并填写 1–8192 UTF-8 字节的纠正说明。');
  }
  return { schema_version: 1, command, run: item.run, kind: item.kind as 'Candidate' | 'Stable',
    id: item.id, event: item.event, snapshot, statement };
}

export function parseCorrectionReceipt(value: unknown, command?: string): CorrectionReceipt {
  const fields = ['schema_version', 'command', 'binding', 'status', 'event', 'candidate'];
  if (!isRecord(value) || Object.keys(value).length !== fields.length || !fields.every((key) => Object.hasOwn(value, key))
      || value.schema_version !== 1 || value.status !== 'Confirmed' || !validId(value.command)
      || (command !== undefined && value.command !== command) || !digest(value.binding)
      || !validId(value.event) || !value.event.startsWith('owner-correction:sha256:') || !value.event.endsWith(':receipt')
      || !validId(value.candidate) || !value.candidate.startsWith('owner-correction-candidate:sha256:')
      || value.event.slice('owner-correction:'.length, -':receipt'.length) !== value.candidate.slice('owner-correction-candidate:'.length)) {
    throw new Error('纠正回执无法与原操作核对，请查询原命令。');
  }
  return value as unknown as CorrectionReceipt;
}

export function parseCorrectionsView(value: unknown, command?: string): CorrectionsView {
  if (!isRecord(value) || value.schema_version !== 1 || !Array.isArray(value.receipts) || value.receipts.length > 100
      || typeof value.truncated !== 'boolean' || (command !== undefined && value.receipts.length > 1)) {
    throw new Error('纠正历史无法验证。');
  }
  const receipts = value.receipts.map((receipt) => parseCorrectionReceipt(receipt, command));
  if (new Set(receipts.map((receipt) => receipt.command)).size !== receipts.length) throw new Error('纠正历史含重复命令。');
  return { schema_version: 1, receipts, truncated: value.truncated };
}
