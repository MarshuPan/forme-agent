import { useEffect, useRef, useState } from 'react';
import type { WorkbenchClient } from '../api/client.ts';
import type { WorkbenchItem } from '../workbench.ts';
import { materialMessages } from '../memory-material.ts';
import type { MemoryMaterial as Material } from '../memory-material.ts';

export function MemoryMaterial({ api, item, snapshot }: {
  api: WorkbenchClient; item: WorkbenchItem; snapshot: string;
}) {
  const [material, setMaterial] = useState<Material | null>(null);
  const [busy, setBusy] = useState(false), [error, setError] = useState('');
  const active = useRef(true);
  useEffect(() => { active.current = true; return () => { active.current = false; }; }, []);
  async function load() {
    if (busy) return;
    setBusy(true); setError('');
    try {
      const view = await api.memoryMaterial(item, snapshot);
      if (active.current) setMaterial(view);
    } catch (problem) {
      if (active.current) setError(problem instanceof Error ? problem.message : '来源材料读取失败。');
    } finally { if (active.current) setBusy(false); }
  }
  return <div aria-busy={busy}>
    {!material && <button className="text-button" disabled={busy} onClick={() => void load()}>{busy ? '正在核对来源…' : '查看来源材料正文'}</button>}
    {error && <p className="form-error" role="alert">{error}</p>}
    {material && <section aria-label="来源材料"><p className="muted small">{materialMessages[material.state]}</p>
      {material.state === 'Available' && <pre>{material.text}</pre>}
      <button className="text-button" onClick={() => { setMaterial(null); setError(''); }}>关闭正文</button>
    </section>}
  </div>;
}
