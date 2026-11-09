import { useEffect, useRef, useState } from 'react';
import type { WorkbenchClient } from '../api/client.ts';
import type { PendingApproval } from '../types.ts';
import { consentReady, desktopPendingRef } from '../desktop.ts';
import type { DesktopCatalog, DesktopConsentView, DesktopConsentRequest } from '../desktop.ts';

export function DesktopConsent({ api, item, ready }: { api: WorkbenchClient; item: PendingApproval; ready: (view: DesktopConsentView | null) => void }) {
  const [view, setView] = useState<DesktopConsentView | null>(null);
  const [catalog, setCatalog] = useState<DesktopCatalog | null>(null);
  const [selected, setSelected] = useState('');
  const [password, setPassword] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [pendingRef, setPendingRef] = useState('');
  const sent = useRef<DesktopConsentRequest | null>(null);
  const running = useRef(false);
  const [submitted, setSubmitted] = useState(false);
  useEffect(() => {
    let active = true;
    void (async () => {
      try {
        const pending = await desktopPendingRef(item);
        const current = await api.desktopConsent(pending);
        if (!active) return;
        setPendingRef(pending); setView(current); ready(current);
      } catch (problem) {
        if (active) { ready(null); setError(problem instanceof Error ? problem.message : '暂时无法读取桌面授权。'); }
      }
    })();
    return () => { active = false; ready(null); };
  }, [api, item.approval_id, item.plan_digest, ready]);
  useEffect(() => {
    if (!consentReady(view)) return;
    const timer = setTimeout(() => { ready(null); setView((current) => current ? { ...current, status: 'expired' } : null); }, Math.max(0,view!.expires_at-Date.now()));
    return () => clearTimeout(timer);
  }, [view, ready]);
  async function query() {
    if (running.current) return;
    running.current = true; setBusy(true); setError('');
    try {
      const pending = pendingRef || await desktopPendingRef(item);
      const current = await api.desktopConsent(pending);
      setPendingRef(pending); setView(current); ready(current);
    } catch (problem) { setView(null); ready(null); setError(problem instanceof Error ? problem.message : '尚未核实授权结果。'); }
    finally { running.current = false; setBusy(false); }
  }
  async function sessions() {
    if (running.current || submitted || view?.status !== 'prepared') return;
    running.current = true; setBusy(true); setError('');
    try { setCatalog(await api.desktopSessions()); setSelected(''); }
    catch (problem) { setError(problem instanceof Error ? problem.message : '无法读取可用会话。'); }
    finally { running.current = false; setBusy(false); }
  }
  async function consent() {
    if (running.current || sent.current || !catalog || !view || view.status !== 'prepared' || !password
        || !catalog.entries.some((entry) => entry.available && entry.session_ref === selected)) return;
    if (catalog.expires_at <= Date.now()) { setError('会话目录已过期，请刷新后重新选择。'); return; }
    const request: DesktopConsentRequest = {
      schema_version:1, operation_id:view.operation_id, expected_revision:catalog.catalog_revision, pending_ref:pendingRef,
      catalog_revision:catalog.catalog_revision, catalog_digest:catalog.catalog_digest, session_ref:selected,
      expires_at:Math.min(view.expires_at,item.expires_at,Date.now()+15*60_000),
    };
    sent.current=request; running.current=true; setSubmitted(true); setBusy(true); setError('');
    const secret=password; setPassword('');
    try {
      const current=await api.consentDesktop(request,secret);
      setView(current); ready(current);
    } catch (problem) { ready(null); setError(problem instanceof Error ? problem.message : '授权结果尚未确认，请核实原操作。'); }
    finally { running.current=false; setBusy(false); }
  }
  return <section aria-label="桌面会话授权">
    <p>这项操作需要你选择一个已登录的桌面会话。</p>
    {view?.status === 'prepared' && !submitted && <>
      <button className="secondary-button" disabled={busy} onClick={() => void sessions()}>{catalog ? '刷新会话目录' : '查看可用桌面'}</button>
      {catalog && <>
        <label>操作哪个桌面<select value={selected} disabled={busy} onChange={(event) => setSelected(event.target.value)}>
          <option value="">请选择桌面会话</option>
          {catalog.entries.filter((entry) => entry.available).map((entry,index) => <option key={entry.session_ref} value={entry.session_ref}>{entry.label}{catalog.entries.length > 1 ? ` · ${index+1}` : ''}</option>)}
        </select></label>
        {!catalog.entries.some((entry) => entry.available) && <p>当前没有可授权的桌面。请登录并解锁设备后刷新。</p>}
        <label>确认账户口令<input type="password" autoComplete="current-password" value={password} disabled={busy} maxLength={1024} onChange={(event) => setPassword(event.target.value)} /></label>
        <button className="secondary-button" disabled={busy || !selected || !password} onClick={() => void consent()}>授权这次操作使用所选桌面</button>
      </>}
    </>}
    {consentReady(view) && <p role="status">桌面授权已确认，可以批准下面这次操作。</p>}
    {view?.status === 'revoked' && <p role="status">这次桌面授权已撤销。</p>}
    {view?.status === 'expired' && <p role="status">这次桌面授权已过期。</p>}
    {(submitted || !view || view.status === 'unknown') && !consentReady(view) && <>
      <p>先核实原授权结果，再继续审批。</p>
      <button className="secondary-button" disabled={busy} onClick={() => void query()}>核实桌面授权</button>
    </>}
    {error && <p role="alert" className="form-error">{error}</p>}
  </section>;
}
