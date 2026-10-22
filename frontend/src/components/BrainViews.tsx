import { useEffect, useRef, useState } from 'react';
import type { FormEvent } from 'react';
import { ApiError, newKey } from '../api/client.ts';
import type { WorkbenchClient } from '../api/client.ts';
import type { RunSummary } from '../types.ts';
import type { ReviewDecision, ReviewState, WorkbenchItem, WorkbenchReview, WorkbenchSection, WorkbenchSnapshot } from '../workbench.ts';
import { reviewLabels, stateLabels } from '../workbench.ts';
import { statusLabel } from './Conversation.tsx';

function useBrainView(api: WorkbenchClient, section: WorkbenchSection) {
  const [snapshot, setSnapshot] = useState<WorkbenchSnapshot | null>(null);
  const [error, setError] = useState(''), [busy, setBusy] = useState(true), [revision, setRevision] = useState(0);
  useEffect(() => {
    let active = true; setBusy(true); setSnapshot(null); setError('');
    void api.workbench(section).then((view) => { if (active) setSnapshot(view); })
      .catch((problem: unknown) => { if (active) setError(problem instanceof Error ? problem.message : '读取失败。'); })
      .finally(() => { if (active) setBusy(false); });
    return () => { active = false; };
  }, [api, section, revision]);
  async function more() {
    if (!snapshot || snapshot.next_offset === null || busy) return;
    setBusy(true); setError('');
    try {
      const next = await api.workbench(section, snapshot.next_offset, snapshot.digest);
      setSnapshot({ ...next, items: [...snapshot.items, ...next.items] });
    } catch (problem) { setError(problem instanceof Error ? problem.message : '分页读取失败，请刷新。'); }
    finally { setBusy(false); }
  }
  return { snapshot, error, busy, more, refresh: () => setRevision((v) => v + 1) };
}

function Evidence({ item, open }: { item: WorkbenchItem; open: (run: string) => void }) {
  return <details><summary>来源、证据与派生关系</summary><dl>
    <div><dt>范围</dt><dd>{item.scope ?? '源事件未声明范围'}</dd></div>
    <div><dt>稳定性</dt><dd>{item.tier ?? '不适用'}</dd></div>
    <div><dt>置信度</dt><dd>{item.confidence === null ? '未声明' : item.confidence.toFixed(2)}</dd></div>
    <div><dt>源事件</dt><dd>{item.event} · #{item.stream_seq}</dd></div>
    <div><dt>时间</dt><dd>{new Date(item.at).toLocaleString()}</dd></div>
  </dl><p>证据 {item.evidence.length} / {item.evidence_count}：</p><ul>{item.evidence.map((ref) => <li key={ref}>{ref}</li>)}</ul>
    <p>根对象及传递派生关系 {item.derived.length} / {item.derived_count}：</p><ul>{item.derived.map((ref) => <li key={ref}>{ref}</li>)}</ul>
    <button className="text-button" onClick={() => open(item.run)}>查看源运行</button>
  </details>;
}

function CandidateControl({ api, item, changed }: { api: WorkbenchClient; item: WorkbenchItem; changed: () => void }) {
  const [decision, setDecision] = useState<ReviewDecision>(item.reviews[0] ?? 'Reject');
  const [busy, setBusy] = useState(false), [message, setMessage] = useState('');
  const pending = useRef<WorkbenchReview | null>(null);
  async function review(event: FormEvent) {
    event.preventDefault(); if (busy) return;
    setBusy(true); setMessage('');
    pending.current ??= { schema_version: 1, command: newKey('review'), run: item.run, candidate: item.id, expected_state: item.state as ReviewState, decision };
    try {
      const receipt = await api.reviewWorkbench(pending.current);
      setMessage(receipt.status === 'Confirmed' ? '主脑已记录处置。' : receipt.status === 'Rejected' ? '主脑拒绝了本次处置；请检查证据、状态和提升规则。' : '原处置未确认，未重复执行。请刷新对象状态并核实原回执。');
      if (receipt.status !== 'Unknown') { pending.current = null; changed(); }
    } catch (problem) {
      setMessage(problem instanceof Error ? problem.message : '处置尚未确认。');
      if (problem instanceof ApiError && problem.status === 409) { pending.current = null; changed(); }
    } finally { setBusy(false); }
  }
  return <form onSubmit={review} aria-busy={busy}>
    <label>候选处置<select value={decision} disabled={busy || pending.current !== null} onChange={(event) => setDecision(event.target.value as ReviewDecision)}>{item.reviews.map((value) => <option key={value} value={value}>{reviewLabels[value]}</option>)}</select></label>
    {decision === 'Retract' && <p className="notice">主脑将撤回该认知并重新评估所有依赖对象。这不删除原始材料。</p>}
    {decision === 'Promote' && <p className="muted small">确认不能代替结果证据，能力或信任提升仍可能被拒绝。</p>}
    <button className="secondary-button" disabled={busy}>{pending.current ? '核实原处置' : '提交处置'}</button>
    {message && <p role="status">{message}</p>}
  </form>;
}

export function CognitionView({ api, open }: { api: WorkbenchClient; open: (run: string) => void }) {
  const [section, setSection] = useState<WorkbenchSection>('Cognition');
  const view = useBrainView(api, section);
  return <section className="view"><header className="view-header"><div><p className="eyebrow">从记录到判断</p><h1>记忆与认知</h1></div><button className="secondary-button" onClick={view.refresh}>刷新</button></header>
    <nav className="run-tabs" aria-label="记忆层次"><button aria-pressed={section === 'Memory'} onClick={() => setSection('Memory')}>原始记忆</button><button aria-pressed={section === 'Cognition'} onClick={() => setSection('Cognition')}>认知、候选与重评</button></nav>
    <p className="muted">读取不会提升候选。撤回、归档与永久删除是不同操作；证据按原运行保留顺序。</p>
    {view.error && <p className="form-error" role="alert">{view.error}</p>}
    {view.busy && <p role="status">正在读取主脑投影…</p>}
    <div className="content-grid">{view.snapshot?.items.map((item) => <article className="surface" key={`${item.run}:${item.kind}:${item.id}`}><p className="eyebrow">{item.kind} · {stateLabels[item.state] ?? item.state}</p><h2>{item.summary}</h2>
      {item.content && <p className="muted">材料引用：{item.content}</p>}
      <Evidence item={item} open={open} />
      {item.reviews.length > 0 && <CandidateControl key={`${item.id}:${item.state}`} api={api} item={item} changed={view.refresh} />}
    </article>)}</div>
    {view.snapshot?.total === 0 && <div className="empty-state"><h2>当前还没有这一层的记录</h2><p>使用中的观察和经过审查的认知会逐步进入这里。</p></div>}
    {view.snapshot && <p className="muted small">已显示 {view.snapshot.items.length} / {view.snapshot.total} 项。</p>}
    {view.snapshot?.next_offset !== null && view.snapshot && <button className="secondary-button" disabled={view.busy} onClick={() => void view.more()}>读取下一页</button>}
  </section>;
}

function LearningResponse({ api, item, changed }: { api: WorkbenchClient; item: WorkbenchItem; changed: () => void }) {
  const [value, setValue] = useState(''), [action, setAction] = useState<'answer' | 'reject' | 'defer'>('answer');
  const [at, setAt] = useState(''), [busy, setBusy] = useState(false), [error, setError] = useState('');
  const pending = useRef<{ action: 'answer' | 'reject' | 'defer'; key: string; body: unknown } | null>(null);
  async function submit(event: FormEvent) {
    event.preventDefault(); if (busy) return;
    setBusy(true); setError('');
    try {
      if (!pending.current) {
        const key = newKey('learning-response');
        const until = at ? new Date(at).getTime() : null;
        if (action === 'defer' && (!until || until <= Date.now())) throw new Error('请选择未来的提醒时间。');
        pending.current = { action, key, body: action === 'answer'
          ? { schema_version: 1, command: key, reply: { schema_version: 1, proposal: item.id, value } }
          : { schema_version: 1, command: key, proposal: item.id, outcome: action === 'reject' ? 'Reject' : 'Defer', defer_until: action === 'defer' ? until : null } };
      }
      const original = pending.current;
      if (original.action === 'answer') await api.answerLearning(original.body, original.key);
      else await api.resolveLearning(original.body, original.key);
      pending.current = null; setValue(''); changed();
    } catch (problem) { setError(problem instanceof Error ? problem.message : '反馈尚未确认；再次提交会使用原操作。'); }
    finally { setBusy(false); }
  }
  return <form onSubmit={submit} aria-busy={busy}><fieldset disabled={busy || pending.current !== null}><legend>回应这次问询</legend>
    <label>回应方式<select value={action} onChange={(event) => setAction(event.target.value as typeof action)}><option value="answer">回答</option><option value="reject">拒绝</option><option value="defer">延后</option></select></label>
    {action === 'answer' && <label>回答内容或已授权资料引用<textarea value={value} onChange={(event) => setValue(event.target.value)} maxLength={4096} required rows={3} /></label>}
    {action === 'defer' && <label>稍后提醒<input type="datetime-local" required value={at} onChange={(event) => setAt(event.target.value)} /></label>}
  </fieldset>{error && <p className="form-error" role="alert">{error}</p>}<button className="secondary-button" disabled={busy}>{pending.current ? '核实原反馈' : '提交反馈'}</button><p className="muted small">回答只会产生受治理候选。</p></form>;
}

export function BrainInbox({ api, runs, open }: { api: WorkbenchClient; runs: RunSummary[]; open: (run: string) => void }) {
  const view = useBrainView(api, 'Inbox');
  const pending = runs.filter((run) => ['Waiting', 'Failed', 'Limited'].includes(run.status));
  return <section className="view"><header className="view-header"><div><p className="eyebrow">需要你参与的事情</p><h1>收件箱</h1></div><button className="secondary-button" onClick={view.refresh}>刷新</button></header>
    <p className="muted">查看不等于采纳或批准。未知结果保留在原运行中，不能通过再次执行来猜测。</p>
    {view.error && <p role="alert" className="form-error">{view.error}</p>}{view.busy && <p role="status">正在读取收件箱…</p>}
    <div className="inbox-list">{pending.map((run) => <button className="inbox-item" key={run.run} onClick={() => open(run.run)}><span><strong>{statusLabel(run.status)}</strong><small>{run.session}</small></span><span>查看审批、等待与恢复 →</span></button>)}</div>
    <div className="content-grid">{view.snapshot?.items.map((item) => <article className="surface" key={`${item.run}:${item.id}`}><p className="eyebrow">{item.kind} · {stateLabels[item.state] ?? item.state}</p><h2>{item.summary}</h2>
      {item.learning && <p>为目标 {item.learning.goal} 确认：{JSON.stringify(item.learning.target)}</p>}
      <Evidence item={item} open={open} />
      {item.kind === 'AskToLearn' && item.state === 'Pending' && <LearningResponse api={api} item={item} changed={view.refresh} />}
    </article>)}</div>
    {view.snapshot?.total === 0 && !pending.length && <div className="empty-state"><h2>目前没有待处理的事情</h2></div>}
    {view.snapshot?.next_offset !== null && view.snapshot && <button className="secondary-button" disabled={view.busy} onClick={() => void view.more()}>读取下一页</button>}
  </section>;
}
