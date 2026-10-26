// Full-sized attachments travel through authenticated Web, signed bridge and
// the production content store. These are not simulated fetch responses.
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { promises as fs } from 'node:fs';

const sha = (value) => `sha256:${createHash('sha256').update(value).digest('hex')}`;

export async function exerciseWebArtifacts(product, coreConfig) {
  const session = await product.request('/api/v1/sessions', { method: 'POST', key: 'session:artifact-process', body: {
    schema_version: 1, command: 'session:artifact-process', title: 'Attachment process fixture', workspace: null,
  } });
  const bytes = Buffer.alloc(4 * 1024 * 1024, 'x');
  bytes.write('repository-owned artifact payload <script>inert</script>');
  const key = 'upload:artifact-process';
  const command = { schema_version: 1, command: key, session: session.session, workspace: null,
    file_name: 'fixture-retained-name.txt', media_type: 'text/plain', bytes_hex: bytes.toString('hex') };
  const receipt = await product.request('/api/v1/brain/artifacts/upload', { method: 'POST', key, body: command });
  assert.equal(receipt.bytes, bytes.length);
  assert.equal(receipt.content_digest, sha(bytes));
  assert.deepEqual(await product.request('/api/v1/brain/artifacts/upload', { method: 'POST', key, body: command }), receipt);
  await product.request('/api/v1/brain/artifacts/upload', { method: 'POST', key,
    body: { ...command, bytes_hex: '6869' }, expected: 400 });
  await product.request('/api/v1/brain/artifacts/upload', { method: 'POST', key,
    body: { ...command, session: 'session:other' }, expected: 400 });
  // Ordinary control JSON retains the small public ingress ceiling.
  await product.request('/api/v1/brain/workbench/query', { method: 'POST',
    body: { schema_version: 1, padding: 'z'.repeat(9000) }, expected: 413, text: true });
  await product.request('/api/v1/brain/artifacts/upload', { method: 'POST', key,
    body: { ...command, bytes_hex: '00'.repeat(4 * 1024 * 1024 + 1) }, expected: 400 });
  const query = { schema_version: 1, session: session.session,
    page: { schema_version: 1, offset: 0, limit: 50, snapshot: null } };
  const first = await product.request('/api/v1/brain/artifacts/query', { method: 'POST', body: query });
  assert.equal(first.total, 1);
  assert.equal(first.artifacts[0].file_name, command.file_name);
  assert.equal(first.artifacts[0].media_type, command.media_type);
  assert.equal(first.artifacts[0].content_digest, receipt.content_digest);
  const download = `/api/v1/brain/artifacts/${encodeURIComponent(receipt.content_ref)}`;
  const verify = async () => {
    const current = await product.request('/api/v1/brain/artifacts/query', { method: 'POST', body: query });
    const original = current.artifacts.find((artifact) => artifact.content_ref === receipt.content_ref);
    assert.ok(original, 'original attachment must remain queryable after later uploads');
    assert.equal(original.file_name, command.file_name);
    assert.equal(original.media_type, command.media_type);
    assert.equal(original.content_digest, receipt.content_digest);
    const downloaded = await product.request(`${download}?session=${encodeURIComponent(session.session)}`, { text: true });
    assert.equal(Buffer.byteLength(downloaded), bytes.length);
    assert.equal(sha(downloaded), receipt.content_digest);
  };
  await verify();
  await product.request(`${download}?session=session%3Awrong`, { expected: 400 });
  const companion = `content:${sha(`workbench-artifact-metadata-v1:${receipt.content_ref}`)}`;
  await product.request(`/api/v1/brain/artifacts/${encodeURIComponent(companion)}?session=${encodeURIComponent(session.session)}`, { expected: 400 });

  const hostile = Buffer.from('<!doctype html><svg><script>window.__forme_pwned = true</script></svg>');
  const hostileKey = 'upload:artifact-hostile-name';
  const hostileCommand = { ...command, command: hostileKey, file_name: '..\\..\\outside.svg',
    media_type: 'image/svg+xml', bytes_hex: hostile.toString('hex') };
  const hostileReceipt = await product.request('/api/v1/brain/artifacts/upload', {
    method: 'POST', key: hostileKey, body: hostileCommand,
  });
  const hostileDownload = `/api/v1/brain/artifacts/${encodeURIComponent(hostileReceipt.content_ref)}?session=${encodeURIComponent(session.session)}`;
  const hostileResponse = await product.request(hostileDownload, { raw: true });
  assert.equal(hostileResponse.headers.get('content-type'), 'application/octet-stream');
  assert.equal(hostileResponse.headers.get('content-disposition'), 'attachment');
  assert.equal(hostileResponse.headers.get('cache-control'), 'no-store');
  assert.deepEqual(Buffer.from(await hostileResponse.arrayBuffer()), hostile);
  const afterHostile = await product.request('/api/v1/brain/artifacts/query', { method: 'POST', body: query });
  assert.equal(afterHostile.total, 2);
  assert.ok(afterHostile.artifacts.some((artifact) => artifact.content_ref === hostileReceipt.content_ref));
  await product.request(`${hostileDownload}&scope=other`, { expected: 400 });
  await product.request(`/api/v1/brain/artifacts/${encodeURIComponent(hostileReceipt.content_ref)}?session=session%3Awrong`, { expected: 400 });
  const stored = await fs.readFile(coreConfig.brain.store_path);
  assert.equal(stored.includes(Buffer.from(command.file_name)), false);
  assert.equal(stored.includes(Buffer.from('repository-owned artifact payload')), false);
  assert.equal(stored.includes(Buffer.from(hostileCommand.file_name)), false);
  assert.equal(stored.includes(hostile), false);
  return { session: session.session, verify };
}
