import type { ArtifactCatalogEntry } from './types.ts';

/** Verify the downloaded plaintext against the authenticated catalog receipt. */
export async function verifyArtifact(blob: Blob, entry: ArtifactCatalogEntry, session: string): Promise<void> {
  if (entry.scope !== `workbench-artifact:${session}` || blob.size !== entry.bytes) {
    throw new Error('附件的会话或大小与保存记录不符，已停止下载。');
  }
  const hash = new Uint8Array(await crypto.subtle.digest('SHA-256', await blob.arrayBuffer()));
  const digest = `sha256:${Array.from(hash, (value) => value.toString(16).padStart(2, '0')).join('')}`;
  if (digest !== entry.content_digest) throw new Error('附件内容与保存摘要不符，已停止下载。');
}
