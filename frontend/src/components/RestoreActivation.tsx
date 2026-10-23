import { useEffect, useRef, useState } from 'react';
import type { FormEvent } from 'react';
import { newKey } from '../api/client.ts';
import type { WorkbenchClient } from '../api/client.ts';
import { activationRejectedBeforeDispatch } from '../configuration.ts';
import { maintenanceComplete, restoreCommand } from '../maintenance.ts';
import type { MaintenanceCommand, MaintenanceSnapshot } from '../maintenance.ts';
import type { RestoreCandidate } from '../data.ts';
import type { ProductConfigView, ProductOperation } from '../types.ts';

export function RestoreActivation({ api, candidate, lock }: {
  api: WorkbenchClient; candidate: RestoreCandidate; lock: (locked: boolean) => void;
}) {
  const [view, setView] = useState<MaintenanceSnapshot | null>(null);
  const [config, setConfig] = useState<ProductConfigView | null>(null);
  const [busy, setBusy] = useState(false), [confirmed, setConfirmed] = useState(false), [error, setError] = useState('');
  const [operation, setOperation] = useState<ProductOperation | null>(null), [uncertain, setUncertain] = useState(false);
  const pending = useRef<MaintenanceCommand | null>(null);
  async function load() {
    const [maintenance, configuration] = await Promise.all([api.maintenance(), api.productConfig()]);
    setView(maintenance); setConfig(configuration);
  }
  useEffect(() => {
    let active = true; setConfirmed(false); setView(null); setConfig(null); setError('');
    void Promise.all([api.maintenance(), api.productConfig()]).then(([maintenance, configuration]) => {
      if (active) { setView(maintenance); setConfig(configuration); }
    }).catch((problem: unknown) => { if (active) setError(problem instanceof Error ? problem.message : '维护状态不可用。'); });
    return () => { active = false; };
  }, [api, candidate.candidate_digest]);
  function observed(result: ProductOperation) {
    setOperation(result);
    const complete = maintenanceComplete(result);
    if (complete) pending.current = null;
    setUncertain(!complete); lock(!complete);
  }
  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault(); if (!view || !config || !confirmed || busy || pending.current) return;
    const form = event.currentTarget, password = String(new FormData(form).get('password') ?? '');
    setBusy(true); setError('');
    try {
      pending.current = restoreCommand(candidate, config, view, newKey('restore'), Date.now());
      lock(true);
      observed(await api.maintain(pending.current, password));
    } catch (problem) {
      if (!pending.current || activationRejectedBeforeDispatch(problem)) { pending.current = null; setUncertain(false); lock(false); }
      else { setUncertain(true); lock(true); }
      setError(problem instanceof Error ? problem.message : '切换结果尚未确认，请核实原操作。');
    } finally { form.reset(); setConfirmed(false); setBusy(false); }
  }
  async function probe(id: string) {
    if (busy) return;
    setBusy(true); setError('');
    try { observed(await api.maintenanceOperation(id)); }
    catch (problem) { setError(problem instanceof Error ? problem.message : '原操作暂不可读；不会重复切换。'); }
    finally { setBusy(false); }
  }
  const available = view?.host.allowed_operations.includes('commit_restore') && config?.configured;
  return <article className="surface" aria-label="恢复候选切换确认"><h3>将已验证候选设为主脑</h3>
    <dl><div><dt>候选</dt><dd>{candidate.candidate_ref}</dd></div><div><dt>验证摘要</dt><dd>{candidate.candidate_digest}</dd></div>
      <div><dt>当前配置版本</dt><dd>{config?.active_revision ?? '读取中'}</dd></div></dl>
    <p className="notice">这会停止主脑并切换数据。旧登录会失效，需要重新登录；历史任务不会恢复执行，原数据与切换前备份保留。</p>
    {view && !available && <p className="notice">当前设备未提供可用的恢复切换能力。候选保留，暂不执行切换。</p>}
    <form onSubmit={submit} aria-busy={busy}><fieldset disabled={!available || busy || uncertain}>
      <label className="checkbox-label"><input type="checkbox" checked={confirmed} onChange={(event) => setConfirmed(event.target.checked)} required />我已核对候选与影响，批准这一次主脑切换。</label>
      <label>当前登录密码<input name="password" type="password" autoComplete="current-password" required maxLength={128} /></label>
      <button className="primary-button" disabled={!confirmed}>确认切换主脑</button>
    </fieldset></form>
    {uncertain && <p className="notice">保留原操作 {pending.current?.operation_id}，只核实结果。若登录已失效，重新登录后在设备维护记录中查询该操作。</p>}
    {pending.current && <button type="button" className="secondary-button" disabled={busy} onClick={() => void probe(pending.current!.operation_id)}>核实原切换</button>}
    {operation && <p role="status">原切换 {operation.operation_id}：{maintenanceComplete(operation) ? '已提交，重新登录后核对设备状态。' : '尚未提交，保持待核实。'}</p>}
    <button type="button" className="text-button" disabled={busy} onClick={() => void load().catch((problem: unknown) => setError(problem instanceof Error ? problem.message : '维护状态不可用。'))}>刷新设备状态</button>
    {error && <p role="alert" className="form-error">{error}</p>}
  </article>;
}
