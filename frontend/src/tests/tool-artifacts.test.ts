import assert from 'node:assert/strict';
import { WorkbenchClient } from '../api/client.ts';
import test from 'node:test';
import { createHash } from 'node:crypto';
import { readToolArtifact, toolArtifactCatalog, validateToolArtifact } from '../tool-artifacts.ts';
import type { ToolArtifactEntry } from '../tool-artifacts.ts';
const sha = (bytes: Uint8Array) => `sha256:${createHash('sha256').update(bytes).digest('hex')}`;
function fixture(size = 600000) {
  // Self-authored binary transport fixture, never presented as a decoded PNG.
  const bytes = new Uint8Array(size); bytes.set([137, 80, 78, 71, 13, 10, 26, 10]);
  const view = new DataView(bytes.buffer); view.setUint32(16, 2); view.setUint32(20, 1);
  for (let i = 33; i < size; i++) bytes[i] = i % 251;
  const entry: ToolArtifactEntry = { schema_version: 2, content_ref: `content:${sha(bytes)}`, content_digest: sha(bytes), action: 'action:test', scope: 'workspace:test', bytes: size, width: 2, height: 1, media_type: 'image/png' };
  let reads = 0;
  const request = async (path: string) => {
    reads++;
    const url = new URL(path, 'http://fixture/'), offset = Number(url.searchParams.get('offset')), count = Number(url.searchParams.get('max_bytes'));
    assert.ok(url.pathname.startsWith('/sessions/session%3Atest/runs/run%3Atest/artifacts/'));
    assert.equal(url.searchParams.get('content_digest'), entry.content_digest);
    const next = offset + count;
    return new Response(bytes.slice(offset, next), { status: 206, headers: { 'Content-Type': 'image/png', 'Cache-Control': 'no-store', 'X-Content-Type-Options': 'nosniff', 'Content-Range': `bytes ${offset}-${next - 1}/${size}`, 'X-Forme-Content-Digest': entry.content_digest, 'X-Forme-Total-Bytes': String(size), 'X-Forme-Next-Offset': String(next) } });
  };
  return { bytes, entry, request, reads: () => reads };
}
test('tool image gathers bounded chunks and checks original digest before returning a Blob', async () => {
  const f = fixture(); const blob = await readToolArtifact(f.request, 'session:test', 'run:test', f.entry, new AbortController().signal);
  assert.deepEqual(new Uint8Array(await blob.arrayBuffer()), f.bytes); assert.equal(blob.type, 'image/png'); assert.equal(f.reads(), 3);
});
test('changed range, identity digest, type, truncation and oversized chunks reject', async () => {
  for (const mode of ['range', 'digest', 'type', 'short', 'long', 'bytes']) {
    const f = fixture(); const request = async (path: string) => {
      const response = await f.request(path); const headers = new Headers(response.headers); let bytes = new Uint8Array(await response.arrayBuffer());
      if (mode === 'range') headers.set('Content-Range', 'bytes 1-262144/600000');
      if (mode === 'digest') headers.set('X-Forme-Content-Digest', sha(new Uint8Array()));
      if (mode === 'type') headers.set('Content-Type', 'text/html');
      if (mode === 'short') bytes = bytes.slice(1);
      if (mode === 'long') { const extra = new Uint8Array(bytes.length + 1); extra.set(bytes); bytes = extra; }
      if (mode === 'bytes') bytes[32] ^= 1;
      return new Response(bytes, { status: 206, headers });
    };
    await assert.rejects(readToolArtifact(request, 'session:test', 'run:test', f.entry, new AbortController().signal));
  }
});
test('cancelled reads release body reader and never request another chunk', async () => {
  const f = fixture(), controller = new AbortController(); let cancelled = 0;
  const request = async (path: string) => {
    const good = await f.request(path);
    return new Response(new ReadableStream<Uint8Array>({ pull() { controller.abort(); }, cancel() { cancelled++; } }), { status: 206, headers: good.headers });
  };
  await assert.rejects(readToolArtifact(request, 'session:test', 'run:test', f.entry, controller.signal));
  assert.equal(f.reads(), 1); assert.equal(cancelled, 1);
});
test('catalog binds session, run, snapshot, unique identities and budgets', async () => {
  const f = fixture(70), snapshot = sha(new Uint8Array([1]));
  const catalog = { schema_version: 2, session: 'session:test', run: 'run:test', digest: snapshot, total: 1, next_offset: null, artifacts: [f.entry] };
  const read = (value: unknown, expected: string | null = null) => toolArtifactCatalog(async () => Response.json(value), 'session:test', 'run:test', 0, expected, new AbortController().signal);
  assert.deepEqual(await read(catalog), catalog);
  for (const change of [{ session: 'session:other' }, { run: 'run:other' }, { schema_version: 1 }, { next_offset: 0 }, { total: 20001 }, { artifacts: [{ ...f.entry, bytes: 67108865 }] }, { artifacts: [f.entry, f.entry], total: 2 }]) await assert.rejects(read({ ...catalog, ...change }));
  await assert.rejects(read(catalog, sha(new Uint8Array([2]))));
  for (const change of [{ width: 0 }, { width: 16777216, height: 2 }, { content_ref: 'C:/private.png' }, { media_type: 'image/svg+xml' }]) assert.throws(() => validateToolArtifact({ ...f.entry, ...change }));
});

test('client binds v2 image requests to the live authentication generation', async () => {
  const f = fixture(70); let client: WorkbenchClient; let clearDuringRead = false;
  const transport: typeof fetch = async (input, options) => {
    assert.ok(String(input).startsWith('/api/v2/'));
    assert.equal(options?.credentials, 'same-origin'); assert.equal(options?.cache, 'no-store');
    const response = await f.request(String(input).slice('/api/v2'.length));
    if (clearDuringRead) client.clear();
    return response;
  };
  client = new WorkbenchClient(transport);
  assert.equal((await client.toolArtifact('session:test', 'run:test', f.entry, new AbortController().signal)).size, 70);
  clearDuringRead = true;
  await assert.rejects(client.toolArtifact('session:test', 'run:test', f.entry, new AbortController().signal));
});
