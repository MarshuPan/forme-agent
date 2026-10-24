import { isRecord } from './types.ts';
import type { WorkbenchItem } from './workbench.ts';

export type MaterialState = 'Available' | 'ReferenceOnly' | 'MissingOrErased' | 'TooLarge' | 'NonText';
export interface MemoryMaterial {
  schema_version: 1; node: string; run: string; event: string; scope: string; content_ref: string;
  state: MaterialState; bytes: number | null; content_digest: string | null; text: string | null;
}
export interface MaterialLifecycleTarget {
  schema_version: 1; node: string; run: string; event: string; scope: string; content_ref: string;
  subject: string; lifecycle_version: number;
}

export function parseMaterialLifecycleTarget(value: unknown, item: WorkbenchItem): MaterialLifecycleTarget {
  if (!isRecord(value) || value.schema_version !== 1 || item.kind !== 'Memory'
      || value.node !== item.id || value.run !== item.run || value.event !== item.event
      || value.scope !== item.scope || typeof value.scope !== 'string' || !value.scope
      || value.content_ref !== item.content || typeof value.content_ref !== 'string' || !value.content_ref
      || typeof value.subject !== 'string' || !/^sha256:[0-9a-f]{64}$/.test(value.subject)
      || typeof value.lifecycle_version !== 'number' || !Number.isSafeInteger(value.lifecycle_version)
      || value.lifecycle_version < 0) throw new Error('遗忘范围无法与当前记忆核对，请重新读取来源。');
  return value as unknown as MaterialLifecycleTarget;
}
export const materialMessages: Record<MaterialState, string> = {
  Available: '来源材料正文',
  ReferenceOnly: '此记录只有材料引用，没有可验证的正文登记，或正文已受保留策略限制。',
  MissingOrErased: '正文已不存在或已被数据生命周期阻断。',
  TooLarge: '正文超过 64 KiB 阅读上限，未截断显示。请从原运行的授权产物入口查看。',
  NonText: '该材料不是 UTF-8 文本，请从原运行的授权产物入口查看。',
};

export async function parseMemoryMaterial(value: unknown, item: WorkbenchItem): Promise<MemoryMaterial> {
  const fail = () => new Error('材料响应无法与当前记忆及来源核对，请刷新后重试。');
  if (!isRecord(value) || value.schema_version !== 1 || item.kind !== 'Memory'
      || value.node !== item.id || value.run !== item.run || value.event !== item.event
      || value.scope !== item.scope || typeof value.scope !== 'string'
      || value.content_ref !== item.content || typeof value.content_ref !== 'string'
      || typeof value.state !== 'string' || !Object.hasOwn(materialMessages, value.state)) throw fail();
  const state = value.state as MaterialState;
  if (state === 'ReferenceOnly' || state === 'MissingOrErased') {
    if (value.bytes !== null || value.content_digest !== null || value.text !== null) throw fail();
  } else {
    if (typeof value.bytes !== 'number' || !Number.isSafeInteger(value.bytes) || value.bytes < 0
        || typeof value.content_digest !== 'string' || !/^sha256:[0-9a-f]{64}$/.test(value.content_digest)) throw fail();
    if (state === 'Available') {
      if (typeof value.text !== 'string' || value.text.length > 65_536 || value.bytes > 65_536) throw fail();
      const bytes = new TextEncoder().encode(value.text);
      if (bytes.length !== value.bytes) throw fail();
      const digest = await crypto.subtle.digest('SHA-256', bytes);
      const actual = 'sha256:' + [...new Uint8Array(digest)].map((byte) => byte.toString(16).padStart(2, '0')).join('');
      if (actual !== value.content_digest) throw fail();
    } else if (value.text !== null || state === 'TooLarge' && value.bytes <= 65_536
        || state === 'NonText' && value.bytes > 65_536) throw fail();
  }
  return value as unknown as MemoryMaterial;
}
