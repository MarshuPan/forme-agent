import { test } from 'node:test';
import assert from 'node:assert/strict';
import { WorkbenchClient } from '../api/client.ts';
import { parseOfflineBackupVerification } from '../offline-backup.ts';

const receipt = () => ({ schema_version: 1, request_digest: `sha256:${'a'.repeat(64)}`, config_revision: 2,
  verification: { schema_version: 1, object: 'backup.sqlite', digest: `sha256:${'b'.repeat(64)}`,
    events: 3, created_at: 1, verified: true } });

test('offline backup verification uses only the explicit read route without retry or core fallback', async () => {
  const calls: { path: string; body: unknown }[] = [];
  const client = new WorkbenchClient(async (url, request) => {
    calls.push({ path: String(url), body: JSON.parse(String(request?.body)) });
    return Response.json(receipt());
  });
  const result = await client.verifyOfflineBackup('backup.sqlite', 2);
  assert.equal(result.verification.verified, true);
  assert.deepEqual(calls, [{ path: '/api/v1/product/data/verify', body: { schema_version: 1, source: 'backup.sqlite', config_revision: 2 } }]);
  let attempts = 0;
  const failed = new WorkbenchClient(async () => { attempts++; throw new TypeError('fixture connection lost'); });
  await assert.rejects(failed.verifyOfflineBackup('backup.sqlite', 2));
  assert.equal(attempts, 1);
});

test('offline receipt rejects different revision, object, unverified or restoration payloads', () => {
  assert.equal(parseOfflineBackupVerification(receipt(), 'backup.sqlite', 2).verification.events, 3);
  assert.throws(() => parseOfflineBackupVerification(receipt(), 'other.sqlite', 2));
  assert.throws(() => parseOfflineBackupVerification(receipt(), 'backup.sqlite', 3));
  for (const patch of [{ verified: false }, { schema_version: 2 }, { events: -1 }, { digest: 'wrong' }, { restoration: null }]) {
    const value = receipt(); Object.assign(value.verification, patch);
    assert.throws(() => parseOfflineBackupVerification(value, 'backup.sqlite', 2));
  }
});
