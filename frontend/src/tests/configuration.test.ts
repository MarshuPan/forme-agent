import { test } from 'node:test';
import assert from 'node:assert/strict';
import { activationRejectedBeforeDispatch, draftFromConfig, emptyModel, patchFromDraft } from '../configuration.ts';
import { ApiError, WorkbenchClient } from '../api/client.ts';
import type { ProductConfigView, ProductCoreActivation } from '../types.ts';

test('saved model editing strips readonly fields and does not echo credentials', () => {
  const { credential: _secret, ...model } = emptyModel();
  const config: ProductConfigView = { schema_version: 1, install_ref: 'install:test', owner: 'owner:test',
    active_revision: 1, acknowledged_revision: null, pending_restart: true, configured: true,
    models: [{ ...model, credential_configured: true }], issue_refs: [] };
  const draft = draftFromConfig(config);
  assert.equal(draft[0].credential, '');
  const patch = patchFromDraft(draft);
  assert.equal(Object.hasOwn(patch[0], 'credential_configured'), false);
  assert.equal(Object.hasOwn(patch[0], 'credential'), false);
  draft[0].credential = 'new-fixture-key';
  assert.equal(patchFromDraft(draft)[0].credential, 'new-fixture-key');
  assert.equal(Object.hasOwn(config.models[0], 'credential'), false);
});

test('only explicit Web rejection permits a fresh confirmation; unknown outcomes stay locked', () => {
  for (const [status, code] of [[400, 'InvalidInput'], [401, 'Unauthenticated'], [403, 'CsrfRejected'],
    [409, 'Conflict'], [410, 'Expired'], [429, 'RateLimited']] as const) {
    assert.equal(activationRejectedBeforeDispatch(new ApiError(status, code, 'fixture')), true);
  }
  for (const [status, code] of [[0, 'ConnectionLost'], [502, 'ConfigurationRejected'],
    [409, 'ConfigurationRejected'], [503, 'ServiceUnavailable']] as const) {
    assert.equal(activationRejectedBeforeDispatch(new ApiError(status, code, 'fixture')), false);
  }
});

test('lost activation response is not retried; probe uses a readonly original operation path', async () => {
  const calls: { path: string; init: RequestInit }[] = [];
  const command: ProductCoreActivation = { schema_version: 1, operation_id: 'activation:fixture', expected_revision: 2,
    action: 'restart', requested_deadline: 120000 };
  const api = new WorkbenchClient(async (path, init) => {
    calls.push({ path: String(path), init: init! });
    if (init?.method === 'POST') throw new TypeError('fixture response lost');
    return Response.json({ schema_version: 1, stage: 'committed', pending_stage: null });
  });
  await assert.rejects(api.activateProductConfig(command, 'fixture-password'), (error: unknown) => error instanceof ApiError && error.status === 0);
  assert.equal(calls.length, 1);
  assert.equal(new Headers(calls[0].init.headers).get('Idempotency-Key'), command.operation_id);
  assert.equal(JSON.parse(String(calls[0].init.body)).expected_revision, 2);
  await api.productOperation(command.operation_id);
  assert.equal(calls.length, 2);
  assert.equal(calls[1].path, '/api/v1/product/operations/activation%3Afixture');
  assert.equal(calls[1].init.method, 'GET');
  assert.equal(calls[1].init.body, undefined);
});

test('static validation remains distinct from a real model connection test', async () => {
  const api = new WorkbenchClient(async (_path, init) => {
    assert.equal(JSON.parse(String(init?.body)).revision, 3);
    return Response.json({ schema_version: 1, revision: 3, valid: true, restart_required: true, model_test_status: 'NotRun', issue_refs: [] });
  });
  const report = await api.validateProductConfig(3, 'validate:fixture');
  assert.equal(report.model_test_status, 'NotRun');
  assert.equal(report.restart_required, true);
});
