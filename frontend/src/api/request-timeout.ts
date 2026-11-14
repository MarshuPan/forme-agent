import { isRecord } from '../types.ts';

/** A response budget only. Never edits the command, renews authority or retries. */
export function requestTimeout(path: string, method: string, body: unknown, now = Date.now()): number {
  if (path === 'bootstrap/claim-v2') return 140_000;
  if (path.startsWith('bootstrap/')) return 50_000;
  if (path === 'product/maintenance' && method === 'POST' && isRecord(body) &&
      body.schema_version === 1 && isRecord(body.action) &&
      ['upgrade', 'rollback_compatible_release'].includes(String(body.action.kind)) &&
      typeof body.requested_deadline === 'number' && Number.isSafeInteger(body.requested_deadline) &&
      body.requested_deadline > now) {
    // Host accepts at most 300s; cover reauthentication, connect/write and receipt.
    return Math.min(300_000, body.requested_deadline - now) + 60_000;
  }
  return 28_000;
}
