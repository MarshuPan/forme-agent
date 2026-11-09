import { useEffect, useRef, useState } from 'react';
import type { WorkbenchClient } from '../api/client.ts';
import type { ToolArtifactEntry } from '../tool-artifacts.ts';

interface Props { api: WorkbenchClient; session: string | null; run: string | null; revision: number }

export function ToolArtifacts({ api, session, run, revision }: Props) {
  const [entries, setEntries] = useState<ToolArtifactEntry[]>([]);
  const [preview, setPreview] = useState<{ url: string; entry: ToolArtifactEntry } | null>(null);
  const [busy, setBusy] = useState(false), [reading, setReading] = useState(false), [error, setError] = useState('');
  const [reload, setReload] = useState(0), [page, setPage] = useState<{ next: number | null; digest: string | null }>({ next: null, digest: null });
  const active = useRef<AbortController | null>(null), imageRead = useRef<AbortController | null>(null), url = useRef<string | null>(null);
  const loading = useRef(false);
  function close() {
    imageRead.current?.abort(); imageRead.current = null;
    if (url.current) URL.revokeObjectURL(url.current);
    url.current = null; setPreview(null); setReading(false);
  }
  useEffect(() => {
    const controller = new AbortController(); active.current = controller;
    close(); setEntries([]); setPage({ next: null, digest: null }); setError(''); setBusy(false); loading.current = false;
    async function load() {
      if (!session || !run) return;
      loading.current = true; setBusy(true);
      try {
        const result = await api.toolArtifacts(session, run, 0, null, controller.signal);
        if (!controller.signal.aborted) { setEntries(result.artifacts); setPage({ next: result.next_offset, digest: result.digest }); }
      } catch (problem) {
        if (!controller.signal.aborted) setError(problem instanceof Error ? problem.message : '暂时无法读取任务图片。');
      } finally { if (!controller.signal.aborted) { setBusy(false); loading.current = false; } }
    }
    void load();
    const hidden = () => { if (document.visibilityState === 'hidden') close(); };
    const leave = () => { controller.abort(); close(); };
    document.addEventListener('visibilitychange', hidden); window.addEventListener('pagehide', leave);
    return () => { controller.abort(); close(); document.removeEventListener('visibilitychange', hidden); window.removeEventListener('pagehide', leave); };
  }, [api, session, run, revision, reload]);

  async function more() {
    const controller = active.current;
    if (!controller || controller.signal.aborted || !session || !run || page.next === null || loading.current) return;
    loading.current = true; setBusy(true); close();
    try {
      const result = await api.toolArtifacts(session, run, page.next, page.digest, controller.signal);
      if (!controller.signal.aborted) {
        setEntries((old) => [...old, ...result.artifacts]); setPage({ next: result.next_offset, digest: result.digest });
      }
    } catch (problem) {
      if (!controller.signal.aborted) { setEntries([]); setPage({ next: null, digest: null }); setError(problem instanceof Error ? problem.message : '任务图片列表已变化，请刷新。'); }
    } finally { if (!controller.signal.aborted) { setBusy(false); loading.current = false; } }
  }
  async function show(entry: ToolArtifactEntry) {
    if (!session || !run || !active.current || active.current.signal.aborted) return;
    close(); setError(''); setReading(true);
    const controller = new AbortController(); imageRead.current = controller;
    const signal = AbortSignal.any([controller.signal, active.current.signal, AbortSignal.timeout(120000)]);
    try {
      const blob = await api.toolArtifact(session, run, entry, signal);
      signal.throwIfAborted();
      const next = URL.createObjectURL(blob); url.current = next; setPreview({ url: next, entry });
    } catch (problem) {
      if (!controller.signal.aborted && !active.current?.signal.aborted) setError(problem instanceof Error ? problem.message : '任务图片未能完整读取。');
    } finally { if (imageRead.current === controller) setReading(false); }
  }
  if (!session || !run) return null;
  return <section aria-label="任务图片">
    <div className="button-row"><span className="muted">任务图片</span><button type="button" className="text-button" disabled={busy} onClick={() => { close(); setReload((value) => value + 1); }}>刷新图片</button></div>
    {busy && <p className="muted" role="status">正在读取任务图片…</p>}
    {!busy && !error && entries.length === 0 && <p className="muted small">此运行暂无可查看的图片。</p>}
    {error && <p role="alert" className="form-error">{error}</p>}
    {entries.length > 0 && <ul className="attachment-list">{entries.map((entry, i) => <li key={entry.content_ref}><span>截图 {i + 1}<small>{entry.width} × {entry.height} · {Math.ceil(entry.bytes / 1024)} KiB</small></span><button type="button" className="text-button" disabled={busy || reading} onClick={() => void show(entry)}>查看截图 {i + 1}</button></li>)}</ul>}
    {page.next !== null && !error && <button type="button" className="secondary-button" disabled={busy} onClick={() => void more()}>更多图片</button>}
    {reading && <p role="status">正在读取并校验完整图片… <button type="button" className="text-button" onClick={close}>取消读取</button></p>}
    {preview && <figure className="tool-artifact-preview"><img src={preview.url} alt="任务执行时保存的桌面截图" onError={() => { close(); setError('图片无法显示，请重新读取。'); }} /><figcaption className="button-row"><a className="text-button" href={preview.url} download={`screenshot-${preview.entry.content_digest.slice(-12)}.png`}>下载图片</a><button type="button" className="text-button" onClick={close}>关闭预览</button></figcaption></figure>}
  </section>;
}
