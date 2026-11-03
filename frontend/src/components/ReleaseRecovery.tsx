import { useEffect, useRef, useState } from 'react';
import type { FormEvent } from 'react';
import { WorkbenchClient, newKey } from '../api/client.ts';
import { activationRejectedBeforeDispatch } from '../configuration.ts';
import { releaseRecoveryCommand, releaseRecoveryVerification } from '../release-recovery.ts';
import type { ReleaseRecoveryCandidate, ReleaseRecoveryCommand, ReleaseRecoveryList, ReleaseRecoveryView } from '../release-recovery.ts';
import type { MaintenanceSnapshot } from '../maintenance.ts';

/** Reload only reads the journal. A durable intent always needs a new explicit confirmation. */
export function ReleaseRecovery({ api, snapshot, original, changed }: {
  api: WorkbenchClient; snapshot: MaintenanceSnapshot; original: string | null; changed: () => void;
}) {
  const [list, setList] = useState<ReleaseRecoveryList | null>(null);
  const [candidate, setCandidate] = useState<ReleaseRecoveryCandidate | null>(null);
  const [view, setView] = useState<ReleaseRecoveryView | null>(null);
  const [busy, setBusy] = useState(false), [uncertain, setUncertain] = useState(false), [error, setError] = useState('');
  const pending = useRef<ReleaseRecoveryCommand | null>(null);
  async function load() {
    try {
      const next = await api.releaseRecoveries();
      if (next.owner !== snapshot.owner) throw new Error('恢复记录不属于当前所有者。');
      setList(next);
      const current = next.operations.find((item) => item.request.operation_id === pending.current?.operation_id)
        ?? next.operations.find((item) => item.stage === 'intent')
        ?? next.operations.find((item) => item.request.candidate.original_operation_id === original);
      if (current) { setView(current); setUncertain(false); if (current.stage === 'committed') pending.current = null; }
    } catch (problem) { setError(problem instanceof Error ? problem.message : '恢复记录不可读。'); }
  }
  useEffect(() => { setCandidate(null); void load(); }, [api, snapshot.product_revision, original]);
  async function inspect() {
    if (!original || busy || pending.current || view?.stage === 'intent') return;
    setBusy(true); setError('');
    try {
      const next = await api.releaseRecoveryCandidate(original);
      if (next.owner !== snapshot.owner || next.product_revision !== snapshot.product_revision) throw new Error('设备状态已变化，请先刷新维护状态。');
      setCandidate(next);
    } catch (problem) { setCandidate(null); setError(problem instanceof Error ? problem.message : '当前不能终止升级。'); }
    finally { setBusy(false); }
  }
  async function probe() {
    const id = pending.current?.operation_id ?? view?.request.operation_id;
    if (!id || busy) return;
    setBusy(true); setError('');
    try {
      const result = await api.releaseRecoveryOperation(id);
      if (result.request.owner !== snapshot.owner) throw new Error('恢复记录不属于当前所有者。');
      setView(result); setUncertain(false); setCandidate(null);
      if (result.stage === 'committed') { pending.current = null; changed(); }
      await load();
    } catch (problem) { setError(problem instanceof Error ? problem.message : '原恢复操作尚未核实。'); }
    finally { setBusy(false); }
  }
  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (busy || uncertain || (pending.current && view?.stage !== 'intent')) return;
    const form = event.currentTarget, fields = new FormData(form);
    if (fields.get('confirm') !== 'on') return;
    const password = String(fields.get('password') ?? '');
    setBusy(true); setError('');
    try {
      let result: ReleaseRecoveryView;
      if (view?.stage === 'intent') {
        const current = await api.releaseRecoveries();
        if (current.owner !== snapshot.owner) throw new Error('恢复记录不属于当前所有者。');
        result = await api.verifyReleaseRecovery(releaseRecoveryVerification(view, current.product_revision, Date.now()), password);
      } else {
        if (!candidate || pending.current) return;
        pending.current = releaseRecoveryCommand(candidate, newKey('release-abort'), Date.now());
        result = await api.recoverRelease(pending.current, password);
      }
      setView(result); setCandidate(null); setUncertain(false);
      if (result.stage === 'committed') { pending.current = null; changed(); }
      await load();
    } catch (problem) {
      if (activationRejectedBeforeDispatch(problem) && view?.stage !== 'intent') pending.current = null;
      else setUncertain(true);
      setError(problem instanceof Error ? problem.message : '裁决结果尚未确认，请查询原号。');
    } finally { form.reset(); setBusy(false); }
  }
  if (!original && !view && !error) return null;
  return <section aria-label="升级中断恢复" aria-busy={busy}>
    <h3>升级中断恢复</h3>
    <p className="muted">仅在能核实尚未进入备份和切换时终止本次升级。保留原版本和失败历史，不启动主脑、不重试升级。</p>
    {original && !pending.current && view?.stage !== 'intent' && <button className="secondary-button" disabled={busy} onClick={() => void inspect()}>检查能否终止本次升级</button>}
    {view && <p role="status">恢复操作 {view.request.operation_id}：{view.stage === 'committed'
      ? '裁决已完成；原升级已终止（Aborted），保留原版本。此结果不是升级成功。' : '裁决已记录但尚未完成，请显式核实原裁决。'}</p>}
    {(candidate || view?.stage === 'intent') && <form onSubmit={submit}><fieldset disabled={busy || uncertain}>
      <p>原升级：{candidate?.original_operation_id ?? view?.request.candidate.original_operation_id}</p>
      <label><input type="checkbox" name="confirm" required />确认终止原升级，保留原版本与失败历史</label>
      <label>登录密码<input type="password" name="password" autoComplete="current-password" required maxLength={128} /></label>
      <button className="primary-button">{view?.stage === 'intent' ? '核实原裁决' : '终止本次升级，保留原版本'}</button>
    </fieldset></form>}
    {(pending.current || view) && <button className="secondary-button" disabled={busy} onClick={() => void probe()}>查询原恢复操作</button>}
    {uncertain && <p className="notice">结果尚未确认。保留原恢复号 {pending.current?.operation_id ?? view?.request.operation_id}，只查询，不自动重发。</p>}
    {list && list.operations.length > 0 && <details><summary>近期恢复裁决</summary><ul>{list.operations.map((item) => <li key={item.request.operation_id}>{item.request.operation_id} · {item.stage}</li>)}</ul></details>}
    {error && <p className="form-error" role="alert">{error}</p>}
  </section>;
}
