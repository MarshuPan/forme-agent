import { useEffect, useRef, useState } from 'react';
import type { ChangeEvent, FormEvent } from 'react';
import { ApiError, newKey } from '../api/client.ts';
import type { WorkbenchClient } from '../api/client.ts';
import { emptyEvents, parseEvents, readDelay, reduceEvents } from '../events.ts';
import type { EventState } from '../events.ts';
import { isFinished } from '../types.ts';
import type { ArtifactUploadReceipt, PendingApproval, RunCommand, RunSummary, RuntimeView, WorkbenchSessionRecord } from '../types.ts';
import { Approval } from './Approval.tsx';

interface Props {
  api: WorkbenchClient; runtime: RuntimeView | null; runs: RunSummary[];
  sessions: WorkbenchSessionRecord[]; selected: string | null; select: (run: string | null) => void;
  refresh: () => Promise<void>; refreshSessions: () => Promise<void>;
}

export function Conversation({ api, runtime, runs, sessions, selected, select, refresh, refreshSessions }: Props) {
  const [draft, setDraft] = useState('');
  const [session, setSession] = useState(() => sessions[0]?.session ?? newKey('session'));
  const [sessionTitle, setSessionTitle] = useState('新对话');
  const [pending, setPending] = useState<RunCommand | null>(null);
  const pendingRef = useRef<RunCommand | null>(null);
  const [busy, setBusy] = useState(false);
  const [summary, setSummary] = useState<RunSummary | null>(null);
  const [answer, setAnswer] = useState<string | null>(null);
  const [events, setEvents] = useState<EventState>(() => emptyEvents(''));
  const [approvals, setApprovals] = useState<PendingApproval[]>([]);
  const [attachments, setAttachments] = useState<ArtifactUploadReceipt[]>([]);
  const [attachmentBusy, setAttachmentBusy] = useState(false);
  const [error, setError] = useState('');
  const [revision, setRevision] = useState(0);
  const cancelOperation = useRef(newKey('cancel'));

  useEffect(() => {
    if (!sessions.length) return;
    if (!sessions.some((item) => item.session === session)) setSession(sessions[0].session);
    const current = sessions.find((item) => item.session === session) ?? sessions[0];
    setSessionTitle(current.title);
  }, [sessions, session]);

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
    let activeSession = session;
    if (!sessions.some((item) => item.session === activeSession)) {
      const created = await api.createWorkbenchSession(draft.trim().slice(0, 64) || '新对话', null);
      activeSession = created.session; setSession(activeSession); setSessionTitle(created.title); await refreshSessions();
    }
    const command = pendingRef.current ?? {
      schema_version: 1 as const, source: 'UserTurn' as const, session: activeSession,
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
  async function upload(event: ChangeEvent<HTMLInputElement>) {
    const file = event.target.files?.[0]; event.target.value = '';
    if (!file || attachmentBusy) return;
    setAttachmentBusy(true); setError('');
    try {
      let activeSession = session;
      if (!sessions.some((item) => item.session === activeSession)) {
        const created = await api.createWorkbenchSession(file.name.slice(0, 64) || '新对话', null);
        activeSession = created.session; setSession(activeSession); setSessionTitle(created.title); await refreshSessions();
      }
      const receipt = await api.uploadArtifact(activeSession, null, file);
      setAttachments((current) => [...current, receipt]);
    } catch (problem) { setError(problem instanceof Error ? problem.message : '附件上传结果尚未确认。'); }
    finally { setAttachmentBusy(false); }
  }
  async function downloadAttachment(attachment: ArtifactUploadReceipt) {
    if (!sessions.some((item) => item.session === session)) return;
    try {
      const blob = await api.artifact(attachment.content_ref, session);
      const url = URL.createObjectURL(blob), link = document.createElement('a');
      link.href = url; link.download = attachment.file_name; link.click(); URL.revokeObjectURL(url);
    } catch (problem) { setError(problem instanceof Error ? problem.message : '附件读取结果尚未确认。'); }
  }
  async function renameSession() {
    const current = sessions.find((item) => item.session === session);
    if (!current) return;
    const title = window.prompt('会话名称', current.title)?.trim();
    if (!title || title === current.title) return;
    try { const updated = await api.updateWorkbenchSession(current.session, current.updated_at, { operation: 'rename', title }); setSessionTitle(updated.title); await refreshSessions(); }
    catch (problem) { setError(problem instanceof Error ? problem.message : '会话名称修改尚未确认。'); }
  }
  async function archiveSession() {
    const current = sessions.find((item) => item.session === session);
    if (!current || !window.confirm('归档当前会话？归档不会删除运行记录。')) return;
    try { await api.updateWorkbenchSession(current.session, current.updated_at, { operation: 'archive', archived: true }); await refreshSessions(); setSession(sessions.find((item) => !item.archived && item.session !== current.session)?.session ?? newKey('session')); select(null); }
    catch (problem) { setError(problem instanceof Error ? problem.message : '会话归档尚未确认。'); }
  }
  async function chooseSession(value: string) {
    if (!value) return;
    setSession(value); select(null);
  }
  async function cancel() {
    if (!selected) return;
    try { await api.cancel(selected, cancelOperation.current); setRevision((v) => v + 1); }
    catch (problem) { setError(problem instanceof Error ? problem.message : '停止请求尚未确认。'); }
  }
  const related = runs.filter((run) => run.session === session);
  return <section className="conversation" aria-labelledby="conversation-title">
    <header className="view-header"><div><p className="eyebrow">与你一起推进</p><h1 id="conversation-title">对话</h1></div>
      <div className="button-row"><select aria-label="当前会话" value={sessions.some((item) => item.session === session) ? session : ''} onChange={(event) => void chooseSession(event.target.value)}><option value="">新会话（发送时创建）</option>{sessions.filter((item) => !item.archived).map((item) => <option value={item.session} key={item.session}>{item.title}</option>)}</select><button className="secondary-button" disabled={pending !== null} onClick={() => { select(null); setSession(newKey('session')); setSessionTitle('新对话'); setSummary(null); setAnswer(null); setEvents(emptyEvents('')); }}>新对话</button>{sessions.some((item) => item.session === session) && <><button className="text-button" onClick={() => void renameSession()}>重命名</button><button className="text-button" onClick={() => void archiveSession()}>归档</button></>}
      </div>
    </header>
    {!runtime && <p className="notice" role="status">主脑暂不可用。可以在设置中查看连接状态。</p>}
    {error && <p role="alert" className="form-error">{error}</p>}
    <div className="conversation-layout"><div className="conversation-main">
      {related.length > 1 && <nav className="run-tabs" aria-label="当前会话运行">{related.map((run, i) => <button className={selected === run.run ? 'active' : ''} key={run.run} onClick={() => select(run.run)}>第 {i + 1} 次 · {statusLabel(run.status)}</button>)}</nav>}
      <div className="messages" aria-live="polite" aria-relevant="additions text">
        {!summary && !selected && <div className="empty-state"><h2>{sessionTitle}，想一起做点什么？</h2><p>从一个问题、一项任务，或一个长期项目开始。</p></div>}
        {answer !== null && <article className="answer"><p className="message-label">forme</p><div className="message-text">{answer}</div></article>}
        {summary && answer === null && <p className="muted">{summary.status === 'Waiting' ? '任务正在等待输入、审批或结果核实。' : isFinished(summary.status) ? '本次运行没有可展示的最终回答。' : '主脑正在处理，公开进度见任务详情。'}</p>}
        {approvals.map((item) => <Approval key={item.approval_id} api={api} item={item} changed={() => { setRevision((v) => v + 1); void refresh(); }} />)}
      </div>
      {attachments.length > 0 && <ul className="attachment-list" aria-label="当前会话附件">{attachments.map((attachment, index) => <li key={`${attachment.content_ref}:${index}`}><span>{attachment.file_name}<small>{Math.ceil(attachment.bytes / 1024)} KiB · 已加密保存，会话运行可读取</small></span><button type="button" className="text-button" onClick={() => void downloadAttachment(attachment)}>下载</button></li>)}</ul>}
      <form className="composer" onSubmit={submit}>
        <label className="sr-only" htmlFor="message">给 forme 的消息</label>
        <textarea id="message" value={draft} onChange={(e) => setDraft(e.target.value)} rows={3} maxLength={6000} disabled={pending !== null} placeholder="说说你的想法，或交给 forme 一件事…" required />
        <div className="composer-footer"><span className="muted">{pending ? '保留原操作标识，核实后再开始下一项。' : '任务执行与审批始终由主脑管理。'}</span>
          <label className="secondary-button attachment-button">{attachmentBusy ? '正在上传…' : '添加附件'}<input type="file" onChange={(event) => void upload(event)} disabled={pending !== null || attachmentBusy} /></label>
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
