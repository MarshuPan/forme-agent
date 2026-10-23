import { test } from 'node:test';
import assert from 'node:assert/strict';
import { WorkbenchClient } from '../api/client.ts';
import { parseMemoryMaterial } from '../memory-material.ts';
import type { WorkbenchItem } from '../workbench.ts';

const item: WorkbenchItem = {
  schema_version: 1, id: 'node:one', kind: 'Memory', run: 'run:source', event: 'event:source', stream_seq: 3,
  at: 1, scope: 'scope:source', state: 'Episodic', summary: 'source', content: 'content:source',
  tier: 'working', confidence: 0.4, evidence: [], evidence_count: 0, derived: [], derived_count: 0, reviews: [], learning: null,
};

async function response(text: string) {
  const bytes = new TextEncoder().encode(text);
  const digest = await crypto.subtle.digest('SHA-256', bytes);
  return {
    schema_version: 1, node: item.id, run: item.run, event: item.event, scope: item.scope, content_ref: item.content,
    state: 'Available', bytes: bytes.length, content_digest: 'sha256:' + Buffer.from(digest).toString('hex'), text,
  };
}

test('material reads use only a memory identity and snapshot in the authenticated same-origin body', async () => {
  const original = '<script>globalThis.bad = true</script>\n原始材料';
  const body = await response(original);
  let calls = 0;
  const api = new WorkbenchClient(async (path, request) => {
    calls++;
    assert.equal(path, '/api/v1/brain/workbench/material');
    assert.equal(request?.credentials, 'same-origin');
    assert.equal(request?.cache, 'no-store');
    assert.deepEqual(JSON.parse(String(request?.body)), {
      schema_version: 1, node: item.id, run: item.run, event: item.event, scope: null, snapshot: 'sha256:' + 'a'.repeat(64),
    });
    assert.match(new Headers(request?.headers).get('Idempotency-Key')!, /^material-view:/);
    return Response.json(body);
  });
  assert.equal((await api.memoryMaterial(item, 'sha256:' + 'a'.repeat(64))).text, original);
  assert.equal(calls, 1);
});

test('material response must match node, run, event, scope, ref, schema and exact UTF-8 digest', async () => {
  const original = await response('UTF-8 原文');
  for (const change of [
    { node: 'other' }, { run: 'other' }, { event: 'other' }, { scope: 'other' }, { content_ref: 'other' },
    { schema_version: 2 }, { state: 'constructor' }, { state: 'Unknown' },
    { text: 'different' }, { bytes: original.bytes + 1 }, { content_digest: 'sha256:' + '0'.repeat(64) },
  ]) await assert.rejects(parseMemoryMaterial({ ...original, ...change }, item));
  assert.equal((await parseMemoryMaterial(original, item)).text, 'UTF-8 原文');
});

test('reference, deleted, non-text and oversized responses cannot smuggle partial or internal text', async () => {
  const base = await response('source');
  for (const state of ['ReferenceOnly', 'MissingOrErased']) {
    const redacted = { ...base, state, bytes: null, content_digest: null, text: null };
    assert.equal((await parseMemoryMaterial(redacted, item)).text, null);
    await assert.rejects(parseMemoryMaterial({ ...redacted, text: 'internal companion' }, item));
  }
  for (const state of ['NonText', 'TooLarge']) {
    const redacted = { ...base, state, bytes: state === 'TooLarge' ? 65_537 : 6, text: null };
    assert.equal((await parseMemoryMaterial(redacted, item)).text, null);
    await assert.rejects(parseMemoryMaterial({ ...redacted, text: 'partial' }, item));
  }
  await assert.rejects(parseMemoryMaterial(await response('x'.repeat(65_537)), item));
});

test('logout during material fetch fences the stale owner response', async () => {
  const body = await response('owner data');
  let complete: ((response: Response) => void) | undefined;
  const api = new WorkbenchClient(() => new Promise<Response>((resolve) => { complete = resolve; }));
  const read = api.memoryMaterial(item, 'sha256:' + 'a'.repeat(64));
  api.clear(); complete!(Response.json(body));
  await assert.rejects(read, /登录状态已变化/);
});
