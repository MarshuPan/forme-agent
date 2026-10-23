import { useEffect, useRef, useState } from 'react';
import type { FormEvent } from 'react';
import { ApiError, newKey } from '../api/client.ts';
import type { WorkbenchClient } from '../api/client.ts';
import type { PendingWorkbenchProposals, ReviewDecision, ReviewState, WorkbenchItem, WorkbenchProposal, WorkbenchReview, WorkbenchSection, WorkbenchSnapshot } from '../workbench.ts';
import { inboxItemStatus, pendingProposalFor, reviewLabels, stateLabels } from '../workbench.ts';

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

function localInputTime(timestamp: number): string {
  const local = new Date(timestamp - new Date(timestamp).getTimezoneOffset() * 60_000);
  return Number.isFinite(local.getTime()) ? local.toISOString().slice(0, 16) : '';
}

function ProposalControl({ api, run, proposal, restored, changed }: {
  api: WorkbenchClient; run: string; proposal: string; restored?: WorkbenchProposal; changed: () => void;
}) {
  const [outcome, setOutcome] = useState<WorkbenchProposal['outcome']>(restored?.outcome ?? 'Adopt');
  const [at, setAt] = useState(restored?.defer_until ? localInputTime(restored.defer_until) : '');
  const [busy, setBusy] = useState(false), [message, setMessage] = useState('');
  const pending = useRef<WorkbenchProposal | null>(restored ?? null);
  useEffect(() => {
    if (restored && pending.current === null) {
      pending.current = restored;
      setOutcome(restored.outcome);
      setAt(restored.defer_until ? localInputTime(restored.defer_until) : '');
    }
  }, [restored]);
  async function submit(event: FormEvent) {
    event.preventDefault(); if (busy) return;
    setBusy(true); setMessage('');
    try {
      if (!pending.current) {
        const until = at ? new Date(at).getTime() : null;
        if (outcome === 'Defer' && (!until || until <= Date.now())) throw new Error('请选择未来的提醒时间。');
        pending.current = { schema_version: 1, command: newKey('proposal'), run, proposal, outcome, defer_until: outcome === 'Defer' ? until : null };
      }
      const operation = pending.current;
      if (!operation) throw new Error('原处置命令不可用，请刷新收件箱。');
      const receipt = await api.resolveWorkbenchProposal(operation);
      setMessage(receipt.status === 'Confirmed' ? '主脑已记录提议处置。' : receipt.status === 'Rejected' ? '主脑拒绝了本次处置。' : '原处置未确认，已阻止重复执行。请刷新并核实原回执。');
      if (receipt.status !== 'Unknown') { pending.current = null; changed(); }
    } catch (problem) {
      setMessage(problem instanceof Error ? problem.message : '处置尚未确认。');
      if (problem instanceof ApiError && problem.status === 409) { pending.current = null; changed(); }
    }
    finally { setBusy(false); }
  }
  return <form onSubmit={submit} aria-busy={busy}><fieldset disabled={busy || pending.current !== null}><legend>处理主动提议</legend>
    <label>处理方式<select value={outcome} onChange={(event) => setOutcome(event.target.value as WorkbenchProposal['outcome'])}><option value="Adopt">采纳</option><option value="Reject">拒绝</option><option value="Defer">延后</option></select></label>
    {outcome === 'Defer' && <label>稍后提醒<input type="datetime-local" required value={at} onChange={(event) => setAt(event.target.value)} /></label>}
  </fieldset>{message && <p role="status">{message}</p>}<button className="secondary-button" disabled={busy}>{pending.current ? '核实原处置' : '提交处置'}</button><p className="muted small">采纳不会绕过权限、审批或执行计划。</p></form>;
}

export function BrainInbox({ api, open }: { api: WorkbenchClient; open: (run: string) => void }) {
  const view = useBrainView(api, 'Inbox');
  const [recovery, setRecovery] = useState<PendingWorkbenchProposals | null>(null);
  const [recoveryError, setRecoveryError] = useState('');
  const [recoveryReady, setRecoveryReady] = useState(false);
  const [recoveryRevision, setRecoveryRevision] = useState(0);
  useEffect(() => {
    let active = true;
    setRecoveryReady(false); setRecoveryError('');
    void api.pendingWorkbenchProposals().then((value) => { if (active) setRecovery(value); })
      .catch((problem: unknown) => { if (active) setRecoveryError(problem instanceof Error ? problem.message : '未决处置记录无法验证。'); })
      .finally(() => { if (active) setRecoveryReady(true); });
    return () => { active = false; };
  }, [api, recoveryRevision]);
  const refresh = () => { view.refresh(); setRecoveryRevision((value) => value + 1); };
  const pendingCommands = recovery?.commands ?? [];
  const represented = new Set((view.snapshot?.items ?? [])
    .filter((item) => item.kind === 'Suggestion')
    .map((item) => JSON.stringify([item.run, item.id])));
  const orphaned = pendingCommands.filter((command) => !represented.has(JSON.stringify([command.run, command.proposal])));
  return <section className="view"><header className="view-header"><div><p className="eyebrow">需要你参与的事情</p><h1>收件箱</h1></div><button className="secondary-button" onClick={refresh}>刷新</button></header>
    <p className="muted">查看不等于采纳或批准。运行待决项会打开原运行，不能通过再次执行来猜测结果。</p>
    {view.error && <p role="alert" className="form-error">{view.error}</p>}{view.busy && <p role="status">正在读取收件箱…</p>}
    {!recoveryReady && <p role="status">正在核对未决处置…</p>}{recoveryError && <p role="alert" className="form-error">{recoveryError}</p>}
    <div className="content-grid">{view.snapshot?.items.map((item) => <article className="surface" key={`${item.run}:${item.id}`}><p className="eyebrow">{item.kind} · {inboxItemStatus(item)}</p><h2>{item.summary}</h2>
      {item.learning && <p>为目标 {item.learning.goal} 确认：{JSON.stringify(item.learning.target)}</p>}
      <Evidence item={item} open={open} />
      {item.kind === 'Waiting' && <button className="secondary-button" onClick={() => open(item.run)}>打开原运行 →</button>}
      {item.kind === 'AskToLearn' && item.state === 'Pending' && <LearningResponse api={api} item={item} changed={view.refresh} />}
      {item.kind === 'Suggestion' && (item.state === 'Pending' || pendingProposalFor(item, pendingCommands)) && recoveryReady && !recoveryError
        && <ProposalControl key={`${item.run}:${item.id}:${pendingProposalFor(item, pendingCommands)?.command ?? 'new'}`} api={api} run={item.run} proposal={item.id} restored={pendingProposalFor(item, pendingCommands)} changed={refresh} />}
      {item.kind === 'Suggestion' && item.state === 'Pending' && !recoveryReady && <p role="status">核对未决处置前暂不可提交。</p>}
    </article>)}</div>
    {recoveryReady && !recoveryError && orphaned.map((command) => <article className="surface" key={`proposal-recovery:${command.command}`}>
      <p className="eyebrow">Suggestion · 原处置待核实</p><h2>主动建议处置尚未取得终态回执</h2>
      <p>来源运行：{command.run} · 建议：{command.proposal}</p>
      <ProposalControl key={command.command} api={api} run={command.run} proposal={command.proposal} restored={command} changed={refresh} />
    </article>)}
    {view.snapshot?.total === 0 && recoveryReady && !recoveryError && orphaned.length === 0 && <div className="empty-state"><h2>目前没有待处理的事情</h2></div>}
    {view.snapshot?.next_offset !== null && view.snapshot && <button className="secondary-button" disabled={view.busy} onClick={() => void view.more()}>读取下一页</button>}
  </section>;
}
