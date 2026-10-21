import { useEffect, useRef, useState } from 'react';
import type { FormEvent } from 'react';
import { ApiError, newKey } from '../api/client.ts';
import type { WorkbenchClient } from '../api/client.ts';
import { emptyEvents, parseEvents, readDelay, reduceEvents } from '../events.ts';
import type { EventState } from '../events.ts';
import { isFinished } from '../types.ts';
import type { PendingApproval, RunCommand, RunSummary, RuntimeView } from '../types.ts';
import { Approval } from './Approval.tsx';

interface Props {
  api: WorkbenchClient; runtime: RuntimeView | null; runs: RunSummary[];
  selected: string | null; select: (run: string | null) => void; refresh: () => Promise<void>;
}

export function Conversation({ api, runtime, runs, selected, select, refresh }: Props) {
  const [draft, setDraft] = useState('');
  const [session, setSession] = useState(() => newKey('session'));
  const [pending, setPending] = useState<RunCommand | null>(null);
  const pendingRef = useRef<RunCommand | null>(null);
  const [busy, setBusy] = useState(false);
  const [summary, setSummary] = useState<RunSummary | null>(null);
  const [answer, setAnswer] = useState<string | null>(null);
  const [events, setEvents] = useState<EventState>(() => emptyEvents(''));
  const [approvals, setApprovals] = useState<PendingApproval[]>([]);
  const [error, setError] = useState('');
  const [revision, setRevision] = useState(0);
  const cancelOperation = useRef(newKey('cancel'));

  useEffect(() => {
    if (!selected) { setSummary(null); setEvents(emptyEvents('')); setAnswer(null); setApprovals([]); return; }
    let closed = false, timer: ReturnType<typeof setTimeout> | undefined, attempt = 0;
    let snapshot = emptyEvents(selected);
    setSummary(null); setAnswer(null); setEvents(snapshot); setApprovals([]); setError('');
    async function poll() {
      try {
        const current = await api.run(selected!);
        const incoming = parseEvents(await api.events(selected!, snapshot.watermark));
        let next = reduceEvents(snapshot, incoming);
        if (next.gap) next = reduceEvents(emptyEvents(selected!), parseEvents(await api.events(selected!, 0)));
        if (next.gap) throw new Error('事件暂不连续，正在等待重新同步。');
        const [output, waiting] = await Promise.all([api.answer(selected!), api.approvals(current.session)]);
        if (closed) return;
        snapshot = next; attempt = 0; setEvents(snapshot); setSummary(current); setSession(current.session);
        setAnswer(output.answer); setApprovals(waiting.filter((item) => item.run === selected)); setError('');
        if (!isFinished(current.status)) timer = setTimeout(() => void poll(), current.status === 'Waiting' ? 5000 : 1800);
      } catch (problem) {
        if (closed) return;
        setError(problem instanceof Error ? problem.message : '暂时无法读取运行状态。');
        if (problem instanceof ApiError && problem.status === 401) return;
        timer = setTimeout(() => void poll(), readDelay(problem, attempt++));
      }
    }
    void poll();
    return () => { closed = true; if (timer) clearTimeout(timer); };
  }, [api, selected, revision]);

  async function submit(event: FormEvent) {
    event.preventDefault(); if (busy || !runtime) return;
    // The ref closes a rapid double-submit before React commits disabled state.
    const command = pendingRef.current ?? {
      schema_version: 1 as const, source: 'UserTurn' as const, session,
      agent_profile: runtime.agent_profile, input: draft, budget: null,
      idempotency_key: newKey('command'),
    };
    if (!command.input.trim()) return;
    pendingRef.current = command; setPending(command); setBusy(true); setError('');
    try {
      const run = await api.submit(command);
      pendingRef.current = null; setPending(null); setDraft(''); select(run); await refresh();
    } catch (problem) { setError(problem instanceof Error ? problem.message : '提交结果尚未确认。'); }
    finally { setBusy(false); }
  }
  async function cancel() {
    if (!selected) return;
    try { await api.cancel(selected, cancelOperation.current); setRevision((v) => v + 1); }
    catch (problem) { setError(problem instanceof Error ? problem.message : '停止请求尚未确认。'); }
  }
  const related = runs.filter((run) => run.session === session);
  return <section className="conversation" aria-labelledby="conversation-title">
    <header className="view-header"><div><p className="eyebrow">与你一起推进</p><h1 id="conversation-title">对话</h1></div>
      <button className="secondary-button" disabled={pending !== null} onClick={() => { select(null); setSession(newKey('session')); setSummary(null); setAnswer(null); setEvents(emptyEvents('')); }}>新对话</button>
    </header>
    {!runtime && <p className="notice" role="status">主脑暂不可用。可以在设置中查看连接状态。</p>}
    {error && <p role="alert" className="form-error">{error}</p>}
    <div className="conversation-layout"><div className="conversation-main">
      {related.length > 1 && <nav className="run-tabs" aria-label="当前会话运行">{related.map((run, i) => <button className={selected === run.run ? 'active' : ''} key={run.run} onClick={() => select(run.run)}>第 {i + 1} 次 · {statusLabel(run.status)}</button>)}</nav>}
      <div className="messages" aria-live="polite" aria-relevant="additions text">
        {!summary && !selected && <div className="empty-state"><h2>今天，想一起做点什么？</h2><p>从一个问题、一项任务，或一个长期项目开始。</p></div>}
        {answer !== null && <article className="answer"><p className="message-label">forme</p><div className="message-text">{answer}</div></article>}
        {summary && answer === null && <p className="muted">{summary.status === 'Waiting' ? '任务正在等待输入、审批或结果核实。' : isFinished(summary.status) ? '本次运行没有可展示的最终回答。' : '主脑正在处理，公开进度见任务详情。'}</p>}
        {approvals.map((item) => <Approval key={item.approval_id} api={api} item={item} changed={() => { setRevision((v) => v + 1); void refresh(); }} />)}
      </div>
      <form className="composer" onSubmit={submit}>
        <label className="sr-only" htmlFor="message">给 forme 的消息</label>
        <textarea id="message" value={draft} onChange={(e) => setDraft(e.target.value)} rows={3} maxLength={6000} disabled={pending !== null} placeholder="说说你的想法，或交给 forme 一件事…" required />
        <div className="composer-footer"><span className="muted">{pending ? '保留原操作标识，核实后再开始下一项。' : '任务执行与审批始终由主脑管理。'}</span>
          <button className="primary-button" disabled={busy || !runtime || (!pending && !draft.trim())}>{busy ? '正在提交…' : pending ? '核实原提交' : '发送'}</button></div>
      </form>
    </div><aside className="details-panel" aria-label="任务详情">
      <h2>任务详情</h2><p className="status-pill">{summary ? statusLabel(summary.status) : '尚未选择运行'}</p>
      <dl><div><dt>事件进度</dt><dd>{events.watermark}</dd></div><div><dt>验证与结果</dt><dd>{summary?.result ? '已记录运行结果，请查看详情' : '尚未确认'}</dd></div></dl>
      {summary && !isFinished(summary.status) && <button className="secondary-button" onClick={() => void cancel()}>请求停止任务</button>}
      <p className="muted small">停止请求不代表动作已经停止；以服务器终态为准。</p>
      {events.events.map((event) => <details className="event-row" key={event.event_id}><summary>#{event.stream_seq} · {event.kind}</summary><pre>{JSON.stringify(event.payload, null, 2)}</pre></details>)}
    </aside></div>
  </section>;
}

export function statusLabel(status: RunSummary['status']): string {
  return { Accepted: '已接收', Running: '处理中', Waiting: '等待处理', Complete: '运行已完成', Aborted: '已停止', Failed: '运行失败', Limited: '达到运行边界' }[status];
}
