import { isRecord } from './types.ts';
import type { DataBackup } from './data.ts';

export interface OfflineBackupVerification {
  schema_version: 1; request_digest: string; config_revision: number; verification: DataBackup;
}
const digest = (value: unknown): value is string => typeof value === 'string' && /^sha256:[0-9a-f]{64}$/.test(value);

export function parseOfflineBackupVerification(value: unknown, source: string, revision: number): OfflineBackupVerification {
  if (!isRecord(value) || value.schema_version !== 1 || !digest(value.request_digest)
    || !Number.isSafeInteger(revision) || revision <= 0 || value.config_revision !== revision) {
    throw new Error('离线备份核验回执与当前请求不符。');
  }
  const backup = value.verification;
  if (!isRecord(backup) || backup.schema_version !== 1 || backup.object !== source || !digest(backup.digest)
    || !Number.isSafeInteger(backup.events) || (backup.events as number) < 0
    || !Number.isSafeInteger(backup.created_at) || (backup.created_at as number) < 0
    || backup.verified !== true || backup.restoration !== undefined) {
    throw new Error('备份尚未取得匹配的完整核验回执。');
  }
  return value as unknown as OfflineBackupVerification;
}
