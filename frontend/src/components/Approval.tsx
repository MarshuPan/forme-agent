import { useState } from 'react';
import { newKey } from '../api/client.ts';
import type { WorkbenchClient } from '../api/client.ts';
import type { PendingApproval } from '../types.ts';

export function Approval({ item, api, changed }: { item: PendingApproval; api: WorkbenchClient; changed: () => void }) {
  const [operation] = useState(() => newKey('approval'));
  const [submitted, setSubmitted] = useState<boolean | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const expired = item.expires_at <= Date.now();
  async function decide(granted: boolean) {
    if (busy || expired || (submitted !== null && submitted !== granted)) return;
    setBusy(true); setSubmitted(granted); setError('');
    try { await api.approve(item, granted, operation); changed(); }
    catch (problem) { setError(problem instanceof Error ? problem.message : '审批结果尚未确认。'); }
    finally { setBusy(false); }
  }
  return <article className="approval-card">
    <p className="eyebrow">等待你决定 · {item.risk_level}</p><h3>{item.action_summary}</h3>
    <dl><div><dt>影响范围</dt><dd>{item.affected_resources.join('、') || item.scope}</dd></div>
      <div><dt>有效期至</dt><dd>{new Date(item.expires_at).toLocaleString()}</dd></div></dl>
    <details><summary>执行与撤销边界</summary><pre>{JSON.stringify({ rollback: item.rollback_boundary, permissions: item.requested_permissions, plan: item.plan_digest, action: item.normalized_action_digest }, null, 2)}</pre></details>
    {error && <p role="alert" className="form-error">{error}</p>}
    <div className="button-row">
      <button className="primary-button" disabled={busy || expired || submitted === false} onClick={() => void decide(true)}>{busy ? '正在确认…' : submitted === true ? '核实原批准' : '仅批准这一次'}</button>
      <button className="secondary-button" disabled={busy || expired || submitted === true} onClick={() => void decide(false)}>{submitted === false ? '核实原拒绝' : '拒绝'}</button>
    </div>{expired && <p className="muted">已过期，请刷新审批状态。</p>}
  </article>;
}
