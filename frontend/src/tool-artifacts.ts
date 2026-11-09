import { isRecord } from './types.ts';

export interface ToolArtifactEntry {
  schema_version: 2; content_ref: string; content_digest: string; action: string; scope: string;
  bytes: number; media_type: 'image/png'; width: number; height: number;
}
export interface ToolArtifactCatalog {
  schema_version: 2; session: string; run: string; digest: string; total: number;
  next_offset: number | null; artifacts: ToolArtifactEntry[];
}
export type ArtifactRequest = (path: string, signal: AbortSignal) => Promise<Response>;
const maxBytes = 64 * 1024 * 1024, chunkBytes = 262144;
const digest = /^sha256:[0-9a-f]{64}$/;
const ref = /^content:sha256:[0-9a-f]{64}$/;
const integer = (value: unknown, min: number, max: number): value is number => typeof value === 'number' && Number.isSafeInteger(value) && value >= min && value <= max;
const fail = () => new Error('任务图片与保存记录不一致，已停止读取。');
const base = (session: string, run: string) => `sessions/${encodeURIComponent(session)}/runs/${encodeURIComponent(run)}/artifacts`;

export function validateToolArtifact(value: unknown): asserts value is ToolArtifactEntry {
  if (!isRecord(value) || value.schema_version !== 2 || typeof value.content_ref !== 'string' || !ref.test(value.content_ref)
    || typeof value.content_digest !== 'string' || !digest.test(value.content_digest) || value.media_type !== 'image/png'
    || typeof value.action !== 'string' || !value.action || typeof value.scope !== 'string' || !value.scope
    || !integer(value.bytes, 1, maxBytes) || !integer(value.width, 1, 16777216) || !integer(value.height, 1, 16777216)
    || value.width * value.height > 16777216) throw fail();
}
export async function toolArtifactCatalog(request: ArtifactRequest, session: string, run: string, offset: number, snapshot: string | null, signal: AbortSignal): Promise<ToolArtifactCatalog> {
  const params = new URLSearchParams({ offset: String(offset), limit: '100' });
  if (snapshot !== null) params.set('snapshot', snapshot);
  const response = await request(`${base(session, run)}?${params}`, signal);
  const encoded = await boundedBody(response, 256 * 1024, signal);
  const value: unknown = JSON.parse(new TextDecoder('utf-8', { fatal: true }).decode(encoded));
  signal.throwIfAborted();
  if (!isRecord(value) || value.schema_version !== 2 || value.session !== session || value.run !== run
    || typeof value.digest !== 'string' || !digest.test(value.digest) || (snapshot !== null && snapshot !== value.digest)
    || !integer(value.total, 0, 20000) || !Array.isArray(value.artifacts)
    || value.artifacts.length !== Math.min(100, Math.max(0, value.total - offset))
    || value.next_offset !== (offset + value.artifacts.length < value.total ? offset + value.artifacts.length : null)) throw fail();
  const ids = new Set<string>();
  for (const entry of value.artifacts) { validateToolArtifact(entry); if (ids.has(entry.content_ref)) throw fail(); ids.add(entry.content_ref); }
  return value as unknown as ToolArtifactCatalog;
}

/** Bound each response while streaming; never allocate from unverified headers. */
async function boundedBody(response: Response, limit: number, signal: AbortSignal): Promise<Uint8Array> {
  const reader = response.body?.getReader();
  if (!reader) throw fail();
  const bytes = new Uint8Array(limit);
  let length = 0;
  const abort = () => { void reader.cancel().catch(() => {}); };
  signal.addEventListener('abort', abort, { once: true });
  try {
    for (;;) {
      signal.throwIfAborted();
      const result = await reader.read();
      signal.throwIfAborted();
      if (result.done) break;
      if (length + result.value.length > limit) throw fail();
      bytes.set(result.value, length); length += result.value.length;
    }
    return bytes.slice(0, length);
  } finally {
    signal.removeEventListener('abort', abort);
    await reader.cancel().catch(() => {}); reader.releaseLock();
  }
}

/** Only verified complete bytes may become a Blob URL; reads never execute a tool. */
export async function readToolArtifact(request: ArtifactRequest, session: string, run: string, entry: ToolArtifactEntry, signal: AbortSignal): Promise<Blob> {
  validateToolArtifact(entry);
  signal.throwIfAborted();
  const bytes = new Uint8Array(entry.bytes);
  for (let offset = 0; offset < entry.bytes;) {
    signal.throwIfAborted();
    const count = Math.min(chunkBytes, entry.bytes - offset), next = offset + count;
    const params = new URLSearchParams({ content_digest: entry.content_digest, offset: String(offset), max_bytes: String(count) });
    const response = await request(`${base(session, run)}/${encodeURIComponent(entry.content_ref)}?${params}`, signal);
    const valid = response.status === 206 && response.headers.get('Content-Type') === 'image/png'
      && response.headers.get('X-Content-Type-Options') === 'nosniff' && response.headers.get('Cache-Control') === 'no-store'
      && response.headers.get('Content-Range') === `bytes ${offset}-${next - 1}/${entry.bytes}`
      && response.headers.get('X-Forme-Content-Digest') === entry.content_digest
      && response.headers.get('X-Forme-Total-Bytes') === String(entry.bytes)
      && response.headers.get('X-Forme-Next-Offset') === String(next);
    if (!valid) { await response.body?.cancel(); throw fail(); }
    const part = await boundedBody(response, count, signal);
    if (part.length !== count) throw fail();
    bytes.set(part, offset); offset = next;
  }
  const hash = new Uint8Array(await crypto.subtle.digest('SHA-256', bytes));
  signal.throwIfAborted();
  const actual = `sha256:${Array.from(hash, (byte) => byte.toString(16).padStart(2, '0')).join('')}`;
  const png = [137, 80, 78, 71, 13, 10, 26, 10];
  if (actual !== entry.content_digest || bytes.length < 33 || !png.every((byte, index) => bytes[index] === byte)
    || new DataView(bytes.buffer).getUint32(16) !== entry.width || new DataView(bytes.buffer).getUint32(20) !== entry.height) throw fail();
  return new Blob([bytes], { type: 'image/png' });
}
