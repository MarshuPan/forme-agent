import { test } from 'node:test';
import assert from 'node:assert/strict';
import { ConversationCommands } from '../conversation.ts';
import { WorkbenchClient } from '../api/client.ts';
import { verifyArtifact } from '../artifacts.ts';
import type { ArtifactCatalogEntry } from '../types.ts';

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => { resolve = done; });
  return { promise, resolve };
}

test('new-session creation takes the exclusive command before its first await', async () => {
  const created = deferred<Response>();
  const calls: Array<{ path: string; body: Record<string, unknown> }> = [];
  const api = new WorkbenchClient(async (path, request) => {
    calls.push({ path: String(path), body: JSON.parse(String(request?.body)) });
    return calls.length === 1 ? created.promise : Response.json('run:one');
  });
  const commands = new ConversationCommands();
  const first = commands.submit(api, null, 'original message', 'profile:one');
  assert.equal(commands.busy, true);
  assert.equal(commands.submit(api, null, 'double click', 'profile:other'), null);
  assert.equal(commands.uploadFile(api, null, new File(['no'], 'no.txt')), null);
  assert.equal(calls.length, 1);
  created.resolve(Response.json({ schema_version: 1, session: 'session:new', title: 'original message' }));
  assert.deepEqual(await first, { run: 'run:one', session: 'session:new' });
  assert.equal(calls.length, 2);
  assert.equal(calls[1].body.session, 'session:new');
  assert.equal(calls[1].body.input, 'original message');
  assert.equal(commands.pending, null);
});

test('lost creation and run replies preserve both identities, input and profile', async () => {
  const calls: Array<{ path: string; body: Record<string, unknown>; key: string | null }> = [];
  let create = 0, submit = 0;
  const api = new WorkbenchClient(async (path, request) => {
    const body = JSON.parse(String(request?.body));
    calls.push({ path: String(path), body, key: new Headers(request?.headers).get('Idempotency-Key') });
    if (String(path).endsWith('/sessions')) {
      if (++create === 1) throw new Error('created but response lost');
      return Response.json({ schema_version: 1, session: 'session:retained', title: body.title });
    }
    if (++submit === 1) throw new Error('accepted but response lost');
    return Response.json('run:retained');
  });
  const commands = new ConversationCommands();
  await assert.rejects(commands.submit(api, null, 'first input', 'profile:original')!);
  assert.equal(commands.busy, false); assert.equal(commands.pending, 'turn');
  await assert.rejects(commands.submit(api, 'session:changed', 'changed', 'profile:changed')!);
  assert.deepEqual(calls[0], calls[1]);
  assert.equal(commands.pendingSession, 'session:retained');
  assert.deepEqual(await commands.submit(api, null, 'changed again', 'profile:other'), { run: 'run:retained', session: 'session:retained' });
  assert.deepEqual(calls[2], calls[3]);
  assert.equal(calls[2].body.input, 'first input');
  assert.equal(calls[2].body.agent_profile, 'profile:original');
  assert.equal(create, 2); assert.equal(submit, 2);
});

test('unknown uploads retain the same file, key and session and exclude runs', async () => {
  const calls: Array<{ path: string; body: unknown; key: string | null }> = [];
  const api = new WorkbenchClient(async (path, request) => {
    calls.push({ path: String(path), body: JSON.parse(String(request?.body)), key: new Headers(request?.headers).get('Idempotency-Key') });
    if (calls.length === 1) throw new Error('uploaded but reply lost');
    return Response.json({ schema_version: 1, content_ref: 'content:one' });
  });
  const commands = new ConversationCommands();
  await assert.rejects(commands.uploadFile(api, 'session:one', new File(['original'], 'original.txt', { type: 'text/plain' }))!);
  assert.equal(commands.pending, 'upload'); assert.equal(commands.pendingFilename, 'original.txt');
  assert.equal(commands.submit(api, 'session:one', 'blocked', 'profile:one'), null);
  await commands.uploadFile(api, 'session:wrong', new File(['replacement'], 'wrong.txt'));
  assert.deepEqual(calls[0], calls[1]);
  assert.equal(calls[0].path, '/api/v1/brain/artifacts/upload');
  assert.equal(commands.pending, null);
});

test('sign-out during session creation prevents dispatching a subsequent run', async () => {
  const created = deferred<Response>(); let calls = 0;
  const api = new WorkbenchClient(async () => { calls++; return created.promise; });
  const commands = new ConversationCommands();
  const first = commands.submit(api, null, 'private draft', 'profile:one');
  commands.dispose();
  assert.equal(commands.pendingInput, null);
  created.resolve(Response.json({ schema_version: 1, session: 'session:created', title: 'draft' }));
  await assert.rejects(first!, /已退出/);
  assert.equal(calls, 1);
  assert.equal(commands.submit(api, null, 'later', 'profile:one'), null);
});

test('late responses from a cleared login cannot repopulate the new owner view', async () => {
  const reply = deferred<Response>();
  const api = new WorkbenchClient(async () => reply.promise);
  const pending = api.runs(); api.clear();
  reply.resolve(Response.json([{ schema_version: 1, run: 'run:old-session' }]));
  await assert.rejects(pending, /登录状态已变化/);
});

test('artifact pagination retains the server snapshot and downloaded bytes require its receipt', async () => {
  let path = '', body: Record<string, unknown> = {};
  const api = new WorkbenchClient(async (input, request) => {
    path = String(input); body = JSON.parse(String(request?.body));
    return Response.json({ schema_version: 1, session: 'session:one', digest: 'sha256:snapshot', artifacts: [], next_offset: null });
  });
  await api.artifacts('session:one', 50, 'sha256:snapshot');
  assert.equal(path, '/api/v1/brain/artifacts/query');
  assert.deepEqual(body.page, { schema_version: 1, offset: 50, limit: 50, snapshot: 'sha256:snapshot' });
  const entry: ArtifactCatalogEntry = { schema_version: 1, content_ref: 'content:one', scope: 'workbench-artifact:session:one', bytes: 2,
    content_digest: 'sha256:8f434346648f6b96df89dda901c5176b10a6d83961dd3c1ac88b59b2dc327aa4', file_name: 'a.txt', media_type: 'text/plain' };
  await verifyArtifact(new Blob(['hi']), entry, 'session:one');
  await assert.rejects(verifyArtifact(new Blob(['hi']), entry, 'session:other'), /会话/);
  await assert.rejects(verifyArtifact(new Blob(['no']), entry, 'session:one'), /摘要/);
  await assert.rejects(verifyArtifact(new Blob(['extra']), entry, 'session:one'), /大小/);
});
