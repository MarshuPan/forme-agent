import { useCallback, useEffect, useRef, useState } from 'react';
import type { WorkbenchClient } from '../api/client.ts';
import type { ArtifactCatalogEntry } from '../types.ts';
import { verifyArtifact } from '../artifacts.ts';

interface Props { api: WorkbenchClient; session: string | null; revision: number }

export function SessionArtifacts({ api, session, revision }: Props) {
  const [entries, setEntries] = useState<ArtifactCatalogEntry[]>([]);
  const [next, setNext] = useState<number | null>(null);
  const [snapshot, setSnapshot] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [reload, setReload] = useState(0);
  const generation = useRef(0), reading = useRef(false);

  const read = useCallback(async (offset: number, digest: string | null, epoch: number) => {
    if (!session || reading.current) return;
    reading.current = true; setBusy(true); setError('');
    try {
      const view = await api.artifacts(session, offset, digest);
      if (epoch !== generation.current) return;
      if (view.session !== session || (digest !== null && digest !== view.digest)) throw new Error('附件列表已变化，请重新读取。');
      setEntries((current) => {
        const items = offset === 0 ? [] : current;
        return Array.from(new Map([...items, ...view.artifacts].map((item) => [item.content_ref, item])).values());
      });
      setNext(view.next_offset); setSnapshot(view.digest);
    } catch (problem) {
      if (epoch === generation.current) setError(problem instanceof Error ? problem.message : '附件目录读取未完成。');
    } finally {
      if (epoch === generation.current) { reading.current = false; setBusy(false); }
    }
  }, [api, session]);

  useEffect(() => {
    const epoch = ++generation.current;
    reading.current = false; setEntries([]); setNext(null); setSnapshot(null); setError(''); setBusy(false);
    if (session) void read(0, null, epoch);
    return () => { generation.current++; };
  }, [session, revision, reload, read]);

  async function download(entry: ArtifactCatalogEntry) {
    if (!session) return;
    const epoch = generation.current;
    try {
      const blob = await api.artifact(entry.content_ref, session);
      await verifyArtifact(blob, entry, session);
      if (epoch !== generation.current) return;
      const url = URL.createObjectURL(blob), link = document.createElement('a');
      link.href = url; link.download = entry.file_name ?? `attachment-${entry.content_digest.slice(-12)}.bin`;
      link.click(); setTimeout(() => URL.revokeObjectURL(url), 1000);
    } catch (problem) {
      if (epoch === generation.current) setError(problem instanceof Error ? problem.message : '附件下载未完成。');
    }
  }

  if (!session) return null;
  return <section aria-label="当前会话附件">
    {(entries.length > 0 || error) && <div className="button-row"><span className="muted">会话附件</span><button type="button" className="text-button" disabled={busy} onClick={() => setReload((value) => value + 1)}>刷新附件</button></div>}
    {error && <p role="alert" className="form-error">{error}</p>}
    {busy && <p role="status" className="muted">正在读取附件目录…</p>}
    {entries.length > 0 && <ul className="attachment-list">{entries.map((entry) => <li key={entry.content_ref}><span>{entry.file_name ?? '历史附件（原名称未知）'}<small>{Math.ceil(entry.bytes / 1024)} KiB · {entry.media_type ?? '类型未知'} · 已加密保存</small></span><button type="button" className="text-button" onClick={() => void download(entry)}>下载</button></li>)}</ul>}
    {next !== null && !error && <button type="button" className="secondary-button" disabled={busy} onClick={() => void read(next, snapshot, generation.current)}>更多附件</button>}
  </section>;
}
