import { test } from 'node:test';
import assert from 'node:assert/strict';
import { ApiError, WorkbenchClient } from '../api/client.ts';
import type { RunCommand } from '../types.ts';

const command: RunCommand = { schema_version: 1, source: 'UserTurn', session: 'session:one', agent_profile: 'agent:one', input: 'test', budget: null, idempotency_key: 'command:same' };

test('native browser transport is called without a client receiver', async () => {
  let calls = 0;
  const transport: typeof fetch = async function (this: unknown) {
    assert.equal(this, undefined); calls++;
    return Response.json({ schema_version: 1, claimed: false, setup_needed: true });
  };
  const api = new WorkbenchClient(transport);
  assert.equal((await api.bootstrap()).claimed, false);
  assert.equal(calls, 1);
});

test('unknown mutation outcomes never auto-retry or replace the original key', async () => {
  const calls: RequestInit[] = [];
  const transport: typeof fetch = async (_path, request) => { calls.push(request!); throw new TypeError('fixture disconnect'); };
  const api = new WorkbenchClient(transport);
  await assert.rejects(api.submit(command), (e: unknown) => e instanceof ApiError && e.status === 0);
  assert.equal(calls.length, 1);
  await assert.rejects(api.submit(command));
  assert.equal(calls.length, 2);
  for (const call of calls) assert.equal(new Headers(call.headers).get('Idempotency-Key'), command.idempotency_key);
});
test('unauthenticated response clears the owner view; raw upstream failures stay private', async () => {
  let expired = 0;
  const api = new WorkbenchClient(async () => new Response('private upstream information', { status: 401 }), () => { expired++; });
  await assert.rejects(api.runs(), (e: unknown) => e instanceof ApiError && !e.message.includes('private upstream'));
  assert.equal(expired, 1);
});
test('login CSRF stays in the same-origin request, not a query or persistent store', async () => {
  const calls: { path: string; request: RequestInit }[] = [];
  const api = new WorkbenchClient(async (input, request) => {
    const path = String(input); calls.push({ path, request: request! });
    if (path.endsWith('login-challenge')) return Response.json({ schema_version: 1, csrf: 'challenge:csrf', attempt_ref: 'sha256:fixture-attempt' });
    return Response.json({ schema_version: 1, session: { schema_version: 1, owner: 'owner:fixture' }, csrf: 'session:csrf' });
  });
  await api.login('fixture', 'fixture-password');
  assert.equal(new Headers(calls[1].request.headers).get('X-Forme-Csrf'), 'challenge:csrf');
  assert.equal(new Headers(calls[1].request.headers).get('Idempotency-Key'), 'sha256:fixture-attempt');
  assert.equal(calls[1].path, '/api/v1/auth/login'); assert.equal(calls[1].request.credentials, 'same-origin');
  assert.equal(calls[1].request.cache, 'no-store');
});
test('server Retry-After is preserved and throttled mutations are not repeated', async () => {
  let calls = 0;
  const api = new WorkbenchClient(async () => { calls++; return Response.json({ schema_version: 1, code: 'RateLimited', safe_message: '稍后再试' }, { status: 429, headers: { 'Retry-After': '60' } }); });
  await assert.rejects(api.submit(command), (e: unknown) => e instanceof ApiError && e.retryAfterMs === 60_000);
  assert.equal(calls, 1);
});
