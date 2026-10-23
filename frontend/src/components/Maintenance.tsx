import { useEffect, useRef, useState } from 'react';
import type { FormEvent } from 'react';
import { WorkbenchClient, newKey } from '../api/client.ts';
import { activationRejectedBeforeDispatch } from '../configuration.ts';
import { maintenanceComplete } from '../maintenance.ts';
import type { MaintenanceAction, MaintenanceCommand, MaintenanceSnapshot } from '../maintenance.ts';
import type { ProductOperation } from '../types.ts';

export function Maintenance({ api, refresh }: { api: WorkbenchClient; refresh: () => void }) {
  const [view, setView] = useState<MaintenanceSnapshot | null>(null);
  const [busy, setBusy] = useState(false), [error, setError] = useState('');
  const [choice, setChoice] = useState('stop_core'), [release, setRelease] = useState(''), [digest, setDigest] = useState('');
  const [operation, setOperation] = useState<ProductOperation | null>(null), [uncertain, setUncertain] = useState(false);
  const [history, setHistory] = useState<ProductOperation | null>(null);
  const pending = useRef<MaintenanceCommand | null>(null);
  async function load() {
    try { setView(await api.maintenance()); }
    catch (problem) { setError(problem instanceof Error ? problem.message : '维护状态读取失败。'); }
  }
  useEffect(() => { void load(); }, [api]);
  function observed(result: ProductOperation) {
    setOperation(result);
    if (maintenanceComplete(result)) { pending.current = null; setUncertain(false); }
    else setUncertain(true);
  }
  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault(); if (!view || busy || pending.current) return;
    const form = event.currentTarget, password = String(new FormData(form).get('password') ?? '');
    const action: MaintenanceAction = choice === 'stage_signed_release'
      ? { kind: 'stage_signed_release', release_id: release, release_digest: digest }
      : { kind: choice as 'start_core' | 'stop_core' | 'restart_core' | 'uninstall_plan' };
    pending.current = { schema_version: 1, operation_id: newKey('maintenance'), expected_revision: view.product_revision,
      action, requested_deadline: Date.now() + 120_000 };
    setBusy(true); setError('');
    try { observed(await api.maintain(pending.current, password)); await load(); refresh(); }
    catch (problem) {
      if (activationRejectedBeforeDispatch(problem)) { pending.current = null; setUncertain(false); }
      else setUncertain(true);
      setError(problem instanceof Error ? problem.message : '操作结果尚未确认。');
    } finally { form.reset(); setBusy(false); }
  }
  async function probe() {
    if (busy || !pending.current) return;
    setBusy(true); setError('');
    try { observed(await api.maintenanceOperation(pending.current.operation_id)); await load(); refresh(); }
    catch (problem) { setError(problem instanceof Error ? problem.message : '原操作尚未核实。'); }
    finally { setBusy(false); }
  }
  async function inspectHistory(id: string) {
    if (busy) return;
    setBusy(true); setError('');
    try { setHistory(await api.maintenanceOperation(id)); }
    catch (problem) { setError(problem instanceof Error ? problem.message : '历史操作暂不可读。'); }
    finally { setBusy(false); }
  }
  return <article className="surface"><h2>设备维护</h2>
    <p className="muted">关闭网页后服务继续运行。停止主脑后仍可登录并维护。</p>
    {view && <><dl><div><dt>当前版本</dt><dd>{view.host.active_release}</dd></div><div><dt>服务状态</dt><dd>{view.host.core_health}</dd></div></dl>
      <form onSubmit={submit} aria-busy={busy}><fieldset disabled={busy || uncertain}>
        <label>维护操作<select value={choice} onChange={(event) => setChoice(event.target.value)}>
          <option value="stop_core">停止主脑</option><option value="start_core">启动主脑</option><option value="restart_core">重启主脑</option>
          {view.host.allowed_operations.includes('stage_signed_release') && <option value="stage_signed_release">验证并暂存已签名版本</option>}
          {view.host.allowed_operations.includes('uninstall_plan') && <option value="uninstall_plan">预览卸载范围</option>}
        </select></label>
        {choice === 'stage_signed_release' && <><label>待暂存版本<input value={release} onChange={(event) => setRelease(event.target.value)} required maxLength={128} /></label><label>版本摘要<input value={digest} onChange={(event) => setDigest(event.target.value)} required pattern="sha256:[a-f0-9]{64}" /></label><p className="muted">安装者需先将签名版本放入本机接收目录。暂存不会切换运行版本。</p></>}
        {choice === 'uninstall_plan' && <p className="notice">仅生成范围预览，保留主脑、密钥、备份与项目，不执行卸载。</p>}
        <label>登录密码<input name="password" type="password" autoComplete="current-password" required maxLength={128} /></label>
        <button className="primary-button" disabled={busy || uncertain}>确认当前操作</button>
      </fieldset></form></>}
    <button className="secondary-button" disabled={busy} onClick={() => { setError(''); void load(); }}>刷新维护状态</button>
    {uncertain && <div className="notice"><p>保留原操作以核实结果，不会自动再次执行。</p><code>{pending.current?.operation_id}</code><button className="secondary-button" disabled={busy} onClick={() => void probe()}>核实原操作</button></div>}
    {operation && <p role="status">操作 {operation.operation_id}：{operation.stage}{operation.pending_stage ? ' · 待核实' : ''}</p>}
    {view && <details><summary>近期维护记录</summary><ul className="service-list">{view.operations.map((item) => <li key={item.operation_id}><span>{item.operation_id}</span><span>{item.stage}</span><button type="button" className="text-button" disabled={busy} onClick={() => void inspectHistory(item.operation_id)}>核实此记录</button></li>)}</ul></details>}
    {history && <p role="status">历史操作 {history.operation_id}：{history.stage}{history.pending_stage ? ` · 待核实 ${history.pending_stage}` : ''}。此查询不会执行操作或解除其他待核实命令。</p>}
    {error && <p className="form-error" role="alert">{error}</p>}
  </article>;
}
