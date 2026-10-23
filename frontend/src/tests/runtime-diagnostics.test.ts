import test from 'node:test';
import assert from 'node:assert/strict';
import { WorkbenchClient } from '../api/client.ts';
import { parseRuntimeDoctor } from '../runtime-diagnostics.ts';

const snapshot = () => ({ schema_version: 1, blocking: true, digest: 'sha256:fixture', checks: [
  { schema_version: 1, name: 'model', status: 'ready', reason: null },
  { schema_version: 1, name: 'worker', status: 'blocking', reason: 'reason:isolation-unavailable' },
  { schema_version: 1, name: 'channel', status: 'disabled', reason: 'reason:not-configured' },
  { schema_version: 1, name: 'desktop', status: 'unsupported', reason: 'reason:no-session' },
] });

test('doctor reads one safe same-origin snapshot without running a model or mutation', async () => {
  const calls: Array<{ path: string; init: RequestInit }> = [];
  const api = new WorkbenchClient(async (path, init) => {
    calls.push({ path: String(path), init: init! });
    return Response.json({ ...snapshot(), private_field: 'must-not-be-projected' });
  });
  assert.deepEqual(await api.runtimeDoctor(), snapshot());
  assert.equal(calls.length, 1);
  assert.equal(calls[0].path, '/api/v1/brain/runtime/doctor');
  assert.equal(calls[0].init.method, 'GET');
  assert.equal(calls[0].init.body, undefined);
  assert.equal(calls[0].init.credentials, 'same-origin');
  assert.equal(calls[0].init.cache, 'no-store');
});

test('unknown schemas, states, duplicate checks, or inconsistent blocking cannot look ready', () => {
  const invalid: unknown[] = [null, {}, { ...snapshot(), schema_version: 2 },
    { ...snapshot(), blocking: false }, { ...snapshot(), checks: {} },
    { ...snapshot(), checks: [...snapshot().checks, snapshot().checks[0]] },
    { ...snapshot(), checks: [{ ...snapshot().checks[0], status: 'healthy' }] },
    { ...snapshot(), checks: [{ ...snapshot().checks[0], schema_version: 2 }] },
    { ...snapshot(), checks: [{ ...snapshot().checks[0], reason: { token: 'fixture' } }] },
    { ...snapshot(), checks: Array.from({ length: 257 }, (_, i) => ({ ...snapshot().checks[0], name: `service:${i}` })) }];
  for (const value of invalid) assert.throws(() => parseRuntimeDoctor(value));
});

test('a failed refresh returns failure instead of an old successful doctor snapshot', async () => {
  let reads = 0;
  const api = new WorkbenchClient(async () => ++reads === 1 ? Response.json(snapshot()) : new Response('', { status: 503 }));
  assert.deepEqual(await api.runtimeDoctor(), snapshot());
  await assert.rejects(api.runtimeDoctor());
  assert.equal(reads, 2);
});
