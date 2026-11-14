import { test } from 'node:test';
import assert from 'node:assert/strict';
import { WorkbenchClient, ApiError } from '../api/client.ts';
import { requestTimeout } from '../api/request-timeout.ts';
import type { MaintenanceCommand } from '../maintenance.ts';

const command = (kind: 'upgrade' | 'rollback_compatible_release', deadline: number): MaintenanceCommand => ({
  schema_version: 1, operation_id: 'maintenance:transport-fixture', expected_revision: 99,
  requested_deadline: deadline, action: { kind, release_id: 'fixture-release',
    release_digest: `sha256:${'a'.repeat(64)}`, backup_ref: 'backup:fixture' },
});

test('release response budgets cover preflight, cap input and preserve expired replay', () => {
  for (const kind of ['upgrade', 'rollback_compatible_release'] as const) {
    const body = command(kind, 121_000);
    const original = JSON.stringify(body);
    assert.equal(requestTimeout('product/maintenance', 'POST', body, 1_000), 180_000);
    assert.equal(JSON.stringify(body), original);
    assert.equal(requestTimeout('product/maintenance', 'GET', body, 1_000), 28_000);
    assert.equal(requestTimeout('product/maintenance/operations/id', 'POST', body, 1_000), 28_000);
    assert.equal(requestTimeout('product/maintenance', 'POST', command(kind, Number.MAX_SAFE_INTEGER), 1_000), 360_000);
    for (const deadline of [1, 1_000, NaN, Infinity, 1.5]) {
      assert.equal(requestTimeout('product/maintenance', 'POST', command(kind, deadline), 1_000), 28_000);
    }
  }
  assert.equal(requestTimeout('product/maintenance', 'POST', { ...command('upgrade', 121_000), action: { kind: 'stop_core' } }, 1_000), 28_000);
  assert.equal(requestTimeout('bootstrap/claim-v2', 'POST', {}, 1_000), 140_000);
  assert.equal(requestTimeout('bootstrap/status', 'GET', undefined, 1_000), 50_000);
});

test('client uses the release budget and sends one unchanged command on disconnect', async (t) => {
  const budgets: number[] = [];
  t.mock.method(AbortSignal, 'timeout', (ms: number) => {
    budgets.push(ms); return new AbortController().signal;
  });
  t.mock.method(Date, 'now', () => 1_000);
  const calls: RequestInit[] = [];
  const api = new WorkbenchClient(async (url, request) => {
    assert.equal(url, '/api/v1/product/maintenance');
    calls.push(request!); throw new TypeError('fixture disconnect');
  });
  const body = command('upgrade', 121_000);
  await assert.rejects(api.maintain(body, 'fixture password'), (error: unknown) =>
    error instanceof ApiError && error.code === 'ConnectionLost');
  assert.deepEqual(budgets, [180_000]);
  assert.equal(calls.length, 1);
  assert.equal(new Headers(calls[0].headers).get('Idempotency-Key'), body.operation_id);
  assert.deepEqual(JSON.parse(String(calls[0].body)), { ...body, password: 'fixture password' });
});
