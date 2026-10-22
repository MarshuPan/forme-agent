import { test } from 'node:test';
import assert from 'node:assert/strict';
import { ApiError, WorkbenchClient } from '../api/client.ts';
import { maintenanceComplete } from '../maintenance.ts';
import type { MaintenanceCommand } from '../maintenance.ts';

test('owner maintenance lost response only probes the original operation, without repeating credentials', async () => {
  const calls: { path: string; init: RequestInit }[] = [];
  const command: MaintenanceCommand = { schema_version: 1, operation_id: 'maintain:fixture', expected_revision: 13,
    action: { kind: 'uninstall_plan' }, requested_deadline: 120_000 };
  const api = new WorkbenchClient(async (path, init) => {
    calls.push({ path: String(path), init: init! });
    if (init?.method === 'POST') throw new TypeError('lost response fixture');
    return Response.json({ schema_version: 1, operation_id: command.operation_id, stage: 'committed', pending_stage: null });
  });
  await assert.rejects(api.maintain(command, 'fixture-password'), (error: unknown) => error instanceof ApiError && error.status === 0);
  assert.equal(calls.length, 1);
  assert.equal(new Headers(calls[0].init.headers).get('Idempotency-Key'), command.operation_id);
  const body = JSON.parse(String(calls[0].init.body));
  assert.equal(body.expected_revision, 13);
  assert.equal(Object.hasOwn(body, 'owner'), false);
  assert.deepEqual(body.action, { kind: 'uninstall_plan' });
  const result = await api.maintenanceOperation(command.operation_id);
  assert.equal(calls.length, 2);
  assert.equal(calls[1].path, '/api/v1/product/maintenance/operations/maintain%3Afixture');
  assert.equal(calls[1].init.method, 'GET');
  assert.equal(calls[1].init.body, undefined);
  assert.equal(maintenanceComplete(result), true);
  assert.equal(maintenanceComplete({ ...result, pending_stage: 'checking' }), false);
  assert.equal(maintenanceComplete({ ...result, stage: 'recovery_required' }), false);
});
