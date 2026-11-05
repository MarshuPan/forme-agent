import { useEffect, useRef, useState } from 'react';
import type { FormEvent } from 'react';
import { WorkbenchClient, newKey } from '../api/client.ts';
import { activationRejectedBeforeDispatch } from '../configuration.ts';
import { legacyRecoveryCommand, legacyRecoveryVerification, legacyReviewDigest } from '../legacy-release-recovery.ts';
import type { LegacyRecoveryCommand, LegacyRecoveryList, LegacyRecoveryView, LegacyReleaseReview } from '../legacy-release-recovery.ts';
import type { MaintenanceSnapshot } from '../maintenance.ts';

export function LegacyReleaseRecovery({ api, snapshot, original, changed }: {
  api: WorkbenchClient; snapshot: MaintenanceSnapshot; original: string | null; changed: () => void;
}) {
  const [review, setReview] = useState<LegacyReleaseReview | null>(null), [digest, setDigest] = useState('');
  const [list, setList] = useState<LegacyRecoveryList | null>(null), [view, setView] = useState<LegacyRecoveryView | null>(null);
  const [busy, setBusy] = useState(false), [uncertain, setUncertain] = useState(false), [error, setError] = useState('');
  const pending = useRef<LegacyRecoveryCommand | null>(null);
  const generation = useRef(0);
  async function reportFor(current: LegacyRecoveryView | null, epoch: number) {
    if (!original) throw new Error('请先找回原升级记录。');
    const report = await api.legacyReleaseReview(original);
    const hash = await legacyReviewDigest(report);
    if (report.binding.owner !== snapshot.owner
        || report.binding.journal_seq !== (current?.request.expected_revision ?? snapshot.product_revision)
        || (current && (current.review_digest !== hash || current.request.review_id !== report.binding.review_id))) {
      throw new Error('报告与原决定或设备状态不符，请先刷新维护状态。');
    }
    if (epoch === generation.current) { setReview(report); setDigest(hash); }
  }
  async function load(epoch: number) {
    try {
      const result = await api.legacyReleaseRecoveries();
      if (result.owner !== snapshot.owner) throw new Error('历史恢复记录不属于当前所有者。');
      if (epoch !== generation.current) return;
      setList(result);
      const current = result.operations.find((item) => item.request.operation_id === pending.current?.operation_id)
        ?? result.operations.find((item) => item.stage === 'intent');
      if (current) {
        setView(current); setUncertain(false);
        if (current.stage === 'committed') pending.current = null;
        else await reportFor(current, epoch);
      }
    } catch {
      if (epoch === generation.current) setError('历史审查当前不可用，尚不能形成可执行决定。');
    }
  }
  useEffect(() => {
    const epoch = ++generation.current;
    setReview(null); setDigest(''); setList(null); setError(''); setBusy(false);
    if (!pending.current) setView(null);
    void load(epoch);
    return () => { generation.current++; };
  }, [api, snapshot.owner, snapshot.product_revision, original]);
  async function inspect() {
    if (!original || busy || pending.current || view?.stage === 'intent') return;
    const epoch = generation.current;
    setBusy(true); setError(''); setView(null); setReview(null); setDigest('');
    try {
      await reportFor(null, epoch);
    } catch { if (epoch === generation.current) setError('历史审查不可用或无法核实。需要完整材料、独立审查和兼容的管理程序，不能强制通过。'); }
    finally { if (epoch === generation.current) setBusy(false); }
  }
  async function probe() {
    const id = pending.current?.operation_id ?? view?.request.operation_id;
    if (!id || busy) return;
    const epoch = generation.current;
    setBusy(true); setError('');
    try {
      const result = await api.legacyReleaseRecoveryOperation(id);
      if (result.request.owner !== snapshot.owner) throw new Error('历史恢复记录不属于当前所有者。');
      if (epoch !== generation.current) return;
      setView(result); setUncertain(false); setReview(null); setDigest('');
      if (result.stage === 'committed') { pending.current = null; changed(); }
      await load(epoch);
    } catch { if (epoch === generation.current) setError('原历史恢复操作尚未核实，请保留原号。'); }
    finally { if (epoch === generation.current) setBusy(false); }
  }
  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (busy || uncertain || (pending.current && view?.stage !== 'intent')) return;
    const form = event.currentTarget, fields = new FormData(form);
    if (fields.get('confirm') !== 'on') return;
    const password = String(fields.get('password') ?? '');
    const epoch = generation.current;
    setBusy(true); setError('');
    try {
      let result: LegacyRecoveryView;
      if (view?.stage === 'intent') {
        if (!review || !digest || digest !== view.review_digest) throw new Error('需要先只读找回原决定绑定的报告及期限。');
        const current = await api.legacyReleaseRecoveries();
        if (current.owner !== snapshot.owner) throw new Error('历史恢复记录不属于当前所有者。');
        if (epoch !== generation.current) return;
        const saved = current.operations.find((item) => item.request.operation_id === view.request.operation_id);
        if (!saved || saved.request_digest !== view.request_digest) throw new Error('原决定尚未核实，请先查询原号。');
        if (saved.stage === 'committed') { setView(saved); pending.current = null; changed(); return; }
        result = await api.verifyLegacyReleaseRecovery(legacyRecoveryVerification(saved, current.product_revision, Date.now(), review), password);
      } else {
        if (!review || review.status !== 'verified' || pending.current) return;
        pending.current = legacyRecoveryCommand(review, digest, newKey('legacy-release-abort'), Date.now());
        result = await api.recoverLegacyRelease(pending.current, password);
      }
      if (result.request.owner !== snapshot.owner) throw new Error('历史恢复记录不属于当前所有者。');
      if (epoch !== generation.current) return;
      setView(result); setUncertain(false); setReview(null); setDigest('');
      if (result.stage === 'committed') { pending.current = null; changed(); }
      await load(epoch);
    } catch (problem) {
      if (epoch !== generation.current) return;
      if (activationRejectedBeforeDispatch(problem) && view?.stage !== 'intent') pending.current = null;
      else setUncertain(true);
      setError(problem instanceof Error ? problem.message : '决定结果尚未确认，请查询原号。');
    } finally { form.reset(); if (epoch === generation.current) setBusy(false); }
  }
  if (!original && !view && !list?.operations.length) return null;
  return <section aria-label="历史升级独立审查" aria-busy={busy}>
    <h3>历史升级独立审查</h3>
    <p className="muted">仅供缺少恢复基线的历史升级。依据独立材料审查，不补造基线；终止后保留原版本与失败历史，不重试升级。</p>
    {original && !pending.current && view?.stage !== 'intent' && <button className="secondary-button" disabled={busy || !list} onClick={() => void inspect()}>读取原升级的历史审查</button>}
    {review && <div role="status"><p>审查结果：{review.status === 'verified' ? '材料已核实' : review.status === 'rejected' ? '审查拒绝' : '材料不足，无法核实'}；原升级：{review.binding.original_operation_id}</p>
      {review.issues.length > 0 && <ul>{review.issues.map((issue) => <li key={issue}>{issue}</li>)}</ul>}</div>}
    {view && <p role="status">历史决定 {view.request.operation_id}：{view.stage === 'committed' ? '已完成；原升级已终止（Aborted），不表示升级成功。' : '已记录但未完成，需要明确核实原决定。'}</p>}
    {review?.status === 'verified' && (!view || view.stage === 'intent') && <form onSubmit={submit}><fieldset disabled={busy || uncertain || Date.now() >= review.binding.expires_at}>
      <label><input type="checkbox" name="confirm" required />确认依据独立审查终止原升级，保留原版本与失败历史</label>
      <label>登录密码<input type="password" name="password" autoComplete="current-password" required maxLength={128} /></label>
      <button className="primary-button">{view?.stage === 'intent' ? '核实原历史决定' : '确认历史恢复决定'}</button>
    </fieldset></form>}
    {(pending.current || view) && <button className="secondary-button" disabled={busy} onClick={() => void probe()}>查询原历史恢复操作</button>}
    {uncertain && <p className="notice">结果尚未确认。保留原号 {pending.current?.operation_id ?? view?.request.operation_id}，只查询，不自动重发。</p>}
    {list && list.operations.length > 0 && <details><summary>近期历史恢复决定</summary><ul>{list.operations.map((item) => <li key={item.request.operation_id}>{item.request.operation_id} · {item.stage}</li>)}</ul></details>}
    {error && <p className="form-error" role="alert">{error}</p>}
  </section>;
}
