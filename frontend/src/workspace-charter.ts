import { isRecord, versioned } from './types.ts';
import type { WorkspaceCharterChangedPayload, WorkspaceCharterRecord } from './types.ts';

/** The existing change payload is versioned by its nested charter, not its envelope. */
export function parsePreparedCharter(value: unknown, requested: WorkspaceCharterRecord, expectedVersion: number): WorkspaceCharterChangedPayload {
  if (!isRecord(value) || !isRecord(value.charter)) throw new Error('约束准备结果不完整，未提交保存。');
  if ('schema_version' in value) versioned(value);
  const charter = value.charter;
  versioned(charter);
  const next = expectedVersion + 1;
  const fields = ['workspace', 'goals', 'constraints', 'prohibitions', 'done_contract', 'review_cadence', 'actor'] as const;
  if (!Number.isSafeInteger(expectedVersion) || expectedVersion < 0 || !Number.isSafeInteger(next)
    || value.expected_version !== expectedVersion || value.committed_version !== next || charter.version !== next
    || charter.actor !== 'Owner' || typeof charter.digest !== 'string' || !/^sha256:[0-9a-f]{64}$/.test(charter.digest)
    || fields.some((field) => JSON.stringify(charter[field]) !== JSON.stringify(requested[field]))) {
    throw new Error('约束准备结果与本次工作空间、版本或内容不一致，未提交保存。');
  }
  return value as unknown as WorkspaceCharterChangedPayload;
}

/** Early form feedback only; the core still validates every prepared and final command. */
export function charterRulesError(workspace: string, constraints: string[], prohibitions: string[]): string {
  const count = constraints.length + prohibitions.length;
  if (!count) return '请至少设置一项可执行约束或禁止规则。';
  if (count > 64) return '约束与禁止规则合计不能超过 64 项。';
  let hasScope = false, hasNodeBound = false;
  for (const [rules, prohibition] of [[constraints, false], [prohibitions, true]] as const) {
    for (const [index, rule] of rules.entries()) {
      const at = `${prohibition ? '禁止规则' : '约束'}第 ${index + 1} 行：`;
      if (new TextEncoder().encode(rule).length > 512) return at + '单项不能超过 512 字节。';
      if (rule === 'read-only' || (!prohibition && rule === 'require-review')) continue;
      if (rule.startsWith('deny-capability:') || (!prohibition && rule.startsWith('allow-capability:'))) {
        const argument = rule.slice(rule.indexOf(':') + 1);
        if (!argument || argument.trim() !== argument) return at + '请填写非空、无首尾空格的能力引用。';
        continue;
      }
      if (!prohibition && rule.startsWith('scope:')) {
        const scope = rule.slice(6);
        if (hasScope || (scope !== workspace && !scope.startsWith(workspace + '/') && !scope.startsWith(workspace + ':'))
          || scope.trim() !== scope) return at + '范围只能设置一次，且必须在当前工作空间之内。';
        hasScope = true; continue;
      }
      if (!prohibition && rule.startsWith('max-nodes:')) {
        const argument = rule.slice(10), max = Number(argument);
        if (hasNodeBound || !/^\+?[0-9]+$/.test(argument) || !Number.isInteger(max) || max < 1 || max > 32) {
          return at + '路线节点上限只能设置一次，取值为 1–32 的整数。';
        }
        hasNodeBound = true; continue;
      }
      return at + '请使用下方列出的可执行规则；自然语言说明请写在项目目标与说明中。';
    }
  }
  return '';
}
