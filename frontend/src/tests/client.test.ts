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
test('project and charter mutations keep one server command key and use allowlisted paths', async () => {
  const calls: { path: string; request: RequestInit }[] = [];
  const transport: typeof fetch = async (input, request) => {
    calls.push({ path: String(input), request: request! });
    return Response.json({ schema_version: 1, event: 'event:fixture' });
  };
  const api = new WorkbenchClient(transport);
  const key = 'command:project-register';
  await api.project({ operation: 'execute', route: { route: 'route:fixture' } }, key);
  const charterKey = 'command:charter';
  await api.prepareCharter('workspace:demo', {
    schema_version: 1, workspace: 'workspace:demo', version: 1, goals: [], constraints: [], prohibitions: [], done_contract: null, review_cadence: null, actor: 'Owner', digest: 'sha256:fixture',
  }, 1, charterKey);
  await api.updateCharter('workspace:demo', {
    charter: { schema_version: 1, workspace: 'workspace:demo', version: 2, goals: [], constraints: [], prohibitions: [], done_contract: null, review_cadence: null, actor: 'Owner', digest: 'sha256:fixture' },
    expected_version: 1,
    committed_version: 2,
  }, charterKey);
  assert.equal(calls[0].path, '/api/v1/brain/projects');
  assert.equal(calls[1].path, '/api/v1/brain/workspaces/workspace%3Ademo/charter');
  assert.equal(calls[2].path, '/api/v1/brain/workspaces/workspace%3Ademo/charter');
  const expectedKeys: Array<[number, string]> = [[0, key], [1, charterKey], [2, charterKey]];
  for (const [index, expected] of expectedKeys) {
    assert.equal(new Headers(calls[index].request.headers).get('Idempotency-Key'), expected);
    assert.equal(calls[index].request.credentials, 'same-origin');
  }
  assert.deepEqual(JSON.parse(String(calls[0].request.body)), { schema_version: 1, request: { operation: 'execute', route: { route: 'route:fixture' } } });
});

test('artifact upload and download stay on same-origin scoped routes', async () => {
  const calls: { path: string; request: RequestInit }[] = [];
  const api = new WorkbenchClient(async (input, request) => {
    calls.push({ path: String(input), request: request! });
    if (calls.length === 1) return Response.json({ schema_version: 1, content_ref: 'content:fixture', content_digest: 'sha256:fixture', scope: 'workbench-artifact:session:one', bytes: 2, file_name: 'a.txt', media_type: 'text/plain' });
    return new Response(new Uint8Array([104, 105]), { headers: { 'Content-Type': 'application/octet-stream' } });
  });
  const receipt = await api.uploadArtifact('session:one', null, new File(['hi'], 'a.txt', { type: 'text/plain' }), 'artifact:one');
  assert.equal(receipt.content_ref, 'content:fixture');
  assert.equal(calls[0].path, '/api/v1/brain/artifacts/upload');
  assert.equal(new Headers(calls[0].request.headers).get('Idempotency-Key'), 'artifact:one');
  const body = JSON.parse(String(calls[0].request.body));
  assert.equal(body.bytes_hex, '6869');
  const bytes = new Uint8Array(await (await api.artifact(receipt.content_ref, 'session:one')).arrayBuffer());
  assert.deepEqual([...bytes], [104, 105]);
  assert.equal(calls[1].path, '/api/v1/brain/artifacts/content%3Afixture?session=session%3Aone');
});

test('brain view pages retain their server digest and review controls retain the original binding', async () => {
  const calls: { path: string; request: RequestInit }[] = [];
  const api = new WorkbenchClient(async (input, request) => {
    calls.push({ path: String(input), request: request! });
    return Response.json({ schema_version: 1, status: 'Unknown' });
  });
  await api.workbench('Cognition', 50, 'sha256:original', 'workspace:one');
  assert.equal(calls[0].path, '/api/v1/brain/workbench/query');
  assert.deepEqual(JSON.parse(String(calls[0].request.body)), { schema_version: 1, section: 'Cognition', offset: 50, limit: 50, scope: 'workspace:one', snapshot: 'sha256:original' });
  const review = { schema_version: 1 as const, command: 'review:stable', run: 'run:one', candidate: 'candidate:one', expected_state: 'Candidate' as const, decision: 'Promote' as const };
  for (let i = 0; i < 2; i++) assert.equal((await api.reviewWorkbench(review)).status, 'Unknown');
  for (const call of calls.slice(1)) {
    assert.equal(call.path, '/api/v1/brain/workbench/review');
    assert.equal(new Headers(call.request.headers).get('Idempotency-Key'), review.command);
    const body = JSON.parse(String(call.request.body));
    assert.deepEqual(body, review);
    assert.equal('actor' in body, false); assert.equal('evidence' in body, false); assert.equal('retraction' in body, false);
  }
});

test('data maintenance keeps the exact command and a lost response is probed without password or action replay', async () => {
  const calls: { path: string; request: RequestInit }[] = [];
  const api = new WorkbenchClient(async (input, request) => {
    calls.push({ path: String(input), request: request! });
    if (calls.length === 1) throw new Error('socket closed after dispatch');
    return Response.json({ schema_version: 1, kind: 'Receipt', receipt: null });
  });
  const command = { schema_version: 1 as const, command: 'data:one', action: { Backup: { target: 'backup.sqlite' } } };
  await assert.rejects(() => api.commandData(command, 'fixture password'), ApiError);
  await api.queryData({ Inspect: { command: command.command } });
  assert.equal(calls.length, 2);
  assert.equal(calls[0].path, '/api/v1/data/command');
  assert.deepEqual(JSON.parse(String(calls[0].request.body)), { command, password: 'fixture password' });
  assert.equal(new Headers(calls[0].request.headers).get('Idempotency-Key'), command.command);
  assert.equal(calls[1].path, '/api/v1/data/query');
  assert.deepEqual(JSON.parse(String(calls[1].request.body)), { schema_version: 1, query: { Inspect: { command: command.command } } });
  assert.equal(String(calls[1].request.body).includes('password'), false);
  assert.equal(String(calls[1].request.body).includes('Backup'), false);
});

test('proposal controls retain exact command binding when an outcome is unknown', async () => {
  const calls: { path: string; request: RequestInit }[] = [];
  const api = new WorkbenchClient(async (input, request) => {
    calls.push({ path: String(input), request: request! });
    return Response.json({ schema_version: 1, status: 'Unknown' });
  });
  const control = { schema_version: 1 as const, command: 'proposal:control', run: 'run:one', proposal: 'proposal:one', outcome: 'Defer' as const, defer_until: 100_000 };
  for (let i = 0; i < 2; i++) assert.equal((await api.resolveWorkbenchProposal(control)).status, 'Unknown');
  assert.equal(calls.length, 2);
  for (const call of calls) {
    assert.equal(call.path, '/api/v1/brain/workbench/proposal');
    assert.equal(new Headers(call.request.headers).get('Idempotency-Key'), control.command);
    assert.deepEqual(JSON.parse(String(call.request.body)), control);
  }
});

test('pending proposal recovery is a read-only authenticated query with closed commands', async () => {
  const calls: { path: string; request: RequestInit }[] = [];
  const api = new WorkbenchClient(async (input, request) => {
    calls.push({ path: String(input), request: request! });
    return Response.json({ schema_version: 1, commands: [
      { schema_version: 1, command: 'proposal:original', run: 'run:one', proposal: 'proposal:one', outcome: 'Adopt', defer_until: null },
    ] });
  });
  const result = await api.pendingWorkbenchProposals();
  assert.equal(result.commands[0]?.command, 'proposal:original');
  assert.equal(calls.length, 1);
  assert.equal(calls[0].path, '/api/v1/workbench/proposals/pending');
  assert.equal(calls[0].request.method, 'GET');
  assert.equal(calls[0].request.body, undefined);
});

test('malformed pending proposal recovery fails closed', async () => {
  const api = new WorkbenchClient(async () => Response.json({ schema_version: 1, commands: [{ command: 'unbound' }] }));
  await assert.rejects(() => api.pendingWorkbenchProposals(), /未决建议处置记录无法验证/);
});

test('ambiguous or excessive recovery lists cannot unlock suggestion controls', async () => {
  const command = { schema_version: 1, command: 'proposal:original', run: 'run:one', proposal: 'proposal:one', outcome: 'Adopt', defer_until: null };
  for (const commands of [[command, command], [command, { ...command, command: 'proposal:second' }], Array.from({ length: 257 }, (_, i) => ({ ...command, command: `key:${i}`, proposal: `target:${i}` }))]) {
    const api = new WorkbenchClient(async () => Response.json({ schema_version: 1, commands }));
    await assert.rejects(() => api.pendingWorkbenchProposals(), /未决建议处置记录无法验证/);
  }
});

test('pending reads and owner resolutions use the authenticated run routes and retain one command body', async () => {
  const calls: { path: string; request: RequestInit }[] = [];
  const api = new WorkbenchClient(async (input, request) => {
    calls.push({ path: String(input), request: request! });
    return calls.length === 1
      ? Response.json({ schema_version: 1, run: 'run:one', snapshot_upper_bound: 4, pending: { kind: 'ToolInterrupt', action: 'action:one', checkpoint: 'event:one', probe_hint: 'inspect the original effect' } })
      : new Response(null, { status: 204 });
  });
  const view = await api.runPending('run:one');
  assert.equal(view.pending?.kind, 'ToolInterrupt');
  const command = 'owner-resolution:stable';
  const body = { ResolveToolOutcome: { command, resolution: { schema_version: 1, action: 'action:one', outcome: 'Completed', result_ref: 'receipt:one', failure_ref: null, reason: null, summary: 'owner checked the result' } } };
  await api.controlRun('run:one', body, command);
  await api.controlRun('run:one', body, command);
  assert.equal(calls[0].path, '/api/v1/brain/runs/run%3Aone/pending');
  assert.equal(calls[1].path, '/api/v1/brain/runs/run%3Aone/control');
  assert.equal(calls[2].path, calls[1].path);
  for (const call of calls.slice(1)) {
    assert.equal(new Headers(call.request.headers).get('Idempotency-Key'), command);
    assert.deepEqual(JSON.parse(String(call.request.body)), body);
  }
});
