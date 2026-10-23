import { versioned } from './types.ts';
import type { RuntimeCheck, RuntimeCheckStatus, RuntimeDoctorView } from './types.ts';

const statuses: RuntimeCheckStatus[] = ['ready', 'disabled', 'unsupported', 'blocking'];
const boundedText = (value: unknown, limit: number): value is string =>
  typeof value === 'string' && value.length > 0 && value.length <= limit;

/** Copy only the documented safe projection; unknown states cannot mean ready. */
export function parseRuntimeDoctor(value: unknown): RuntimeDoctorView {
  versioned(value);
  if (!Array.isArray(value.checks) || value.checks.length > 256 ||
      typeof value.blocking !== 'boolean' || !boundedText(value.digest, 256)) {
    throw new Error('运行诊断格式不兼容，请重新检查页面与服务版本。');
  }
  const names = new Set<string>();
  const checks: RuntimeCheck[] = value.checks.map((item: unknown) => {
    versioned(item);
    if (!boundedText(item.name, 256) || names.has(item.name) ||
        !statuses.includes(item.status as RuntimeCheckStatus) ||
        !(item.reason === null || boundedText(item.reason, 2048))) {
      throw new Error('运行诊断包含无法识别的检查结果。');
    }
    names.add(item.name);
    return { schema_version: 1, name: item.name, status: item.status as RuntimeCheckStatus, reason: item.reason };
  });
  if (value.blocking !== checks.some((check) => check.status === 'blocking')) {
    throw new Error('运行诊断的阻断状态不一致。');
  }
  return { schema_version: 1, checks, blocking: value.blocking, digest: value.digest };
}

export function runtimeCheckLabel(status: RuntimeCheckStatus): string {
  return { ready: '已装配', disabled: '未启用', unsupported: '当前不支持', blocking: '阻断' }[status];
}
