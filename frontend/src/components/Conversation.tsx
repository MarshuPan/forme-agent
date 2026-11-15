import { useEffect, useRef, useState } from 'react';
import type { ChangeEvent, FormEvent } from 'react';
import { ApiError, newKey } from '../api/client.ts';
import type { WorkbenchClient } from '../api/client.ts';
import { emptyEvents, readDelay } from '../events.ts';
import { isFinished } from '../types.ts';
import type { PendingApproval, RunPendingView, RunSummary, RuntimeView, WorkbenchSessionRecord } from '../types.ts';
import { Approval } from './Approval.tsx';
import { ConversationCommands } from '../conversation.ts';
import { SessionArtifacts } from './SessionArtifacts.tsx';
import { ToolArtifacts } from './ToolArtifacts.tsx';
import { RunPendingControls } from './RunPendingControls.tsx';
import { runStatusMessage } from '../run-status.ts';
import { SessionActions } from './SessionActions.tsx';
import { useConversationHistory } from '../use-conversation-history.ts';

interface Props {
  active: boolean; api: WorkbenchClient; runtime: RuntimeView | null; runs: RunSummary[];
  sessions: WorkbenchSessionRecord[]; selected: string | null; select: (run: string | null) => void;
  session: string | null; setSession: (session: string | null) => void; newConversation: () => void; draftRevision: number;
  rememberSession: (session: WorkbenchSessionRecord) => void;
  refresh: () => Promise<void>; refreshSessions: () => Promise<void>;
  observed: (run: RunSummary) => void;
  workspaceRevision: number;
  onPendingChange: (pending: boolean) => void;
}

type WorkspaceChange = { session: string; expectedUpdatedAt: number; workspace: string | null; key: string };

export function Conversation({ active, api, runtime, runs, sessions, selected, select, session, setSession, rememberSession, newConversation, draftRevision, refresh, refreshSessions, observed, workspaceRevision, onPendingChange }: Props) {
  const [drafts, setDrafts] = useState<Record<string, string>>({});
  const draftKey = session ?? `draft:${draftRevision}`;
  const draft = drafts[draftKey] ?? '';
  const setDraft = (value: string) => setDrafts((current) => ({ ...current, [draftKey]: value }));
  const [sessionTitle, setSessionTitle] = useState('新对话');
  const [commands] = useState(() => new ConversationCommands(rememberSession));
  const [, redraw] = useState(0);
  const pending = commands.pending, busy = commands.busy;
  const [workspaceOptions, setWorkspaceOptions] = useState<string[]>([]);
  const [workspaceOptionsError, setWorkspaceOptionsError] = useState('');
  const [workspaceChange, setWorkspaceChange] = useState<WorkspaceChange | null>(null);
  const [workspaceBusy, setWorkspaceBusy] = useState(false);
  const [sessionActionsLocked, setSessionActionsLocked] = useState(false);
  const commandLocked = pending !== null || busy || workspaceBusy || workspaceChange !== null;
  const locked = commandLocked || sessionActionsLocked;
  const [artifactRevision, setArtifactRevision] = useState(0);
  const [approvals, setApprovals] = useState<PendingApproval[]>([]);
  const [pendingView, setPendingView] = useState<RunPendingView | null>(null);
  const [error, setError] = useState('');
  const [revision, setRevision] = useState(0);
  const cancelOperations = useRef(new Map<string, string>());
  const alive = useRef(true);
  const history = useConversationHistory(api, session, runs, selected, active, revision, observed);
  const turn = history.turns.find((item) => item.summary.run === selected);
  const summary = turn?.summary ?? null;
  const events = turn?.events ?? emptyEvents('');
  const [visibleTurns, setVisibleTurns] = useState(40);
  const [detailsOpen, setDetailsOpen] = useState(false);

  useEffect(() => { alive.current = true; return () => { alive.current = false; commands.dispose(); }; }, [commands]);
  useEffect(() => { onPendingChange(locked); }, [locked, onPendingChange]);
  useEffect(() => { setVisibleTurns(40); setDetailsOpen(false); setError(''); }, [session, draftRevision]);
  useEffect(() => {
    if (!selected && !history.loading && history.turns.length) select(history.turns.at(-1)!.summary.run);
  }, [selected, history.loading, history.turns, select]);

  useEffect(() => {
    // An unsaved draft must survive history refreshes and another tab's writes.
    const current = sessions.find((item) => item.session === session);
    setSessionTitle(current?.title ?? '新对话');
  }, [sessions, session]);

  useEffect(() => {
    let active = true;
    if (!runtime) {
      setWorkspaceOptions([]);
      setWorkspaceOptionsError('');
      return () => { active = false; };
    }
    setWorkspaceOptions([runtime.workspace]);
    setWorkspaceOptionsError('');
    void api.projects().then((ledger) => {
      if (!active) return;
      const values = new Set([runtime.workspace, ...Object.values(ledger.projects).map((project) => project.workspace)]);
      setWorkspaceOptions([...values].sort((left, right) => left === runtime.workspace ? -1 : right === runtime.workspace ? 1 : left.localeCompare(right)));
    }).catch(() => {
      if (active) setWorkspaceOptionsError('项目工作空间暂不可读；当前只提供默认工作空间。');
    });
    return () => { active = false; };
  }, [api, runtime, workspaceRevision]);

  useEffect(() => {
    setApprovals([]); setPendingView(null);
    if (!selected || !session || !active) return;
    let closed = false, timer: ReturnType<typeof setTimeout> | undefined, attempt = 0;
    async function poll() {
      try {
        const [waiting, pendingState] = await Promise.all([api.approvals(session!), api.runPending(selected!)]);
        if (closed) return;
        if (pendingState.run !== selected || waiting.some((item) => item.session !== session)) throw new Error('任务控制状态的会话绑定不一致。');
        attempt = 0; setApprovals(waiting.filter((item) => item.run === selected)); setPendingView(pendingState);
        if (!summary || !isFinished(summary.status)) timer = setTimeout(() => void poll(), 5000);
      } catch (problem) {
        if (closed) return;
        setError(problem instanceof Error ? problem.message : '暂时无法读取运行状态。');
        if (problem instanceof ApiError && problem.status === 401) return;
        timer = setTimeout(() => void poll(), readDelay(problem, attempt++));
      }
    }
    void poll();
    return () => { closed = true; if (timer) clearTimeout(timer); };
  }, [api, session, selected, active, revision, summary?.status]);

  async function submit(event: FormEvent) {
    event.preventDefault(); if (!runtime || currentSession?.archived || sessionActionsLocked || workspaceBusy || workspaceChange !== null) return;
    const request = commands.submit(api, savedSession(), draft, runtime.agent_profile);
    if (!request) return;
    commandState(); setError('');
    try {
      const result = await request;
      if (!alive.current) return;
      setSession(result.session); setDraft(''); select(result.run);
      try { await Promise.all([refresh(), refreshSessions()]); }
      catch { if (alive.current) setError('消息已提交，列表暂未刷新。可以在当前对话继续交流。'); }
    } catch (problem) { if (alive.current) setError(problem instanceof Error ? problem.message : '提交结果尚未确认。'); }
    finally { if (alive.current) commandState(); }
  }
  // The confirmed session survives a failed or late list refresh.
  function savedSession() { return session; }
  function commandState() { redraw((value) => value + 1); }
  async function upload(file?: File) {
    const request = commands.uploadFile(api, savedSession(), file);
    if (!request) return;
    commandState(); setError('');
    try {
      const result = await request;
      if (!alive.current) return;
      if (result.session !== session) setDrafts((current) => ({ ...current, [result.session]: current[draftKey] ?? '' }));
      setSession(result.session); setArtifactRevision((value) => value + 1); await refreshSessions();
    } catch (problem) { if (alive.current) setError(problem instanceof Error ? problem.message : '附件上传结果尚未确认。'); }
    finally { if (alive.current) commandState(); }
  }
  function chooseFile(event: ChangeEvent<HTMLInputElement>) {
    const file = event.target.files?.[0]; event.target.value = '';
    if (file) void upload(file);
  }
  async function sessionUpdated(updated: WorkbenchSessionRecord) {
    rememberSession(updated);
    if (updated.archived) {
      newConversation();
    } else setSessionTitle(updated.title);
    try { await refreshSessions(); }
    catch { setError('会话更改已确认，但列表刷新失败；重新读取后再进行下一项操作。'); }
  }
  async function persistWorkspaceChange(command: WorkspaceChange) {
    setWorkspaceBusy(true); setError('');
    try {
      await api.updateWorkbenchSession(command.session, command.expectedUpdatedAt, {
        operation: 'associate_workspace', workspace: command.workspace,
      }, command.key);
      setWorkspaceChange((current) => current?.key === command.key ? null : current);
      await refreshSessions();
    } catch (problem) {
      setError(problem instanceof Error ? problem.message : '工作空间关联结果尚未确认。');
      if (problem instanceof ApiError && problem.status >= 400 && problem.status < 500) {
        setWorkspaceChange((current) => current?.key === command.key ? null : current);
        void refreshSessions();
      }
    } finally { setWorkspaceBusy(false); }
  }
  function associateWorkspace(value: string) {
    const current = sessions.find((item) => item.session === session);
    if (!current || locked) return;
    const workspace = value || null;
    if (current.workspace === workspace) return;
    const command = {
      session: current.session,
      expectedUpdatedAt: current.updated_at,
      workspace,
      key: newKey('session-workspace'),
    };
    setWorkspaceChange(command);
    void persistWorkspaceChange(command);
  }
  function verifyWorkspaceChange() {
    if (workspaceChange && !workspaceBusy) void persistWorkspaceChange(workspaceChange);
  }
  async function cancel() {
    if (!selected) return;
    const operation = cancelOperations.current.get(selected) ?? newKey('cancel');
    cancelOperations.current.set(selected, operation);
    try { await api.cancel(selected, operation); setRevision((v) => v + 1); }
    catch (problem) { setError(problem instanceof Error ? problem.message : '停止请求尚未确认。'); }
  }
  const currentSession = sessions.find((item) => item.session === session) ?? null;
  const selectableWorkspaces = [...new Set([...workspaceOptions, ...(currentSession?.workspace ? [currentSession.workspace] : [])])];
  return <section className="conversation" aria-labelledby="conversation-title">
    <header className="view-header"><div><p className="eyebrow">与你一起推进</p><h1 id="conversation-title">{sessionTitle}</h1></div></header>
    {currentSession && <SessionActions key={currentSession.session} api={api} session={currentSession} locked={commandLocked} lock={setSessionActionsLocked} refresh={refreshSessions} confirmed={sessionUpdated} />}
    {currentSession && <div className="session-context-row"><label className="session-context-control">后续运行工作空间<select aria-label="后续运行工作空间" value={currentSession.workspace ?? ''} disabled={locked || workspaceOptions.length === 0} onChange={(event) => associateWorkspace(event.target.value)}><option value="">默认工作空间{runtime ? `（${runtime.workspace}）` : ''}</option>{selectableWorkspaces.map((workspace) => <option key={workspace} value={workspace}>{workspace === runtime?.workspace ? `${workspace}（默认）` : workspace}</option>)}</select></label>{workspaceChange?.session === session && <button className="secondary-button" disabled={workspaceBusy} onClick={verifyWorkspaceChange}>{workspaceBusy ? '正在核实…' : '核实原关联'}</button>}{workspaceOptionsError && <span className="muted small" role="status">{workspaceOptionsError}</span>}</div>}
    {!runtime && <p className="notice" role="status">主脑暂不可用。可以在设置中查看连接状态。</p>}
    {currentSession?.archived && <p className="notice" role="status">当前会话已归档，可以查看历史记录；请新建对话以继续交流。</p>}
    {(error || history.error) && <p role="alert" className="form-error">{error || history.error}<button className="secondary-button" onClick={() => setRevision((value) => value + 1)}>重新读取</button></p>}
    <div className={`conversation-layout ${detailsOpen ? '' : 'without-details'}`}><div className="conversation-main">
      <div className="messages" aria-live="polite" aria-relevant="additions text">
        {history.loading && <p role="status" className="muted">正在读取对话…</p>}
        {!history.loading && !history.turns.length && !selected && <div className="empty-state"><h2>想一起做点什么？</h2><p>从一个问题、一项任务，或一个长期项目开始。</p></div>}
        {history.turns.length > visibleTurns && <button className="secondary-button" onClick={() => setVisibleTurns((count) => count + 40)}>查看更早的消息</button>}
        {history.turns.slice(-visibleTurns).map((item) => <section className="conversation-turn" key={item.summary.run} data-run={item.summary.run} aria-label="对话消息">
          {item.input !== null && <article className="answer user-message"><p className="message-label">你</p><div className="message-text">{item.input}</div></article>}
          <article className="answer"><p className="message-label">forme</p>{item.answer !== null ? <div className="message-text">{item.answer}</div> : <p className="muted">{runStatusMessage(item.summary)}</p>}
            <button className="turn-details-button" disabled={locked} aria-expanded={detailsOpen && selected === item.summary.run} onClick={() => { select(item.summary.run); setDetailsOpen(true); }}>{statusLabel(item.summary.status)} · 查看详情</button>
          </article>
          {item.summary.run === selected && <>{approvals.map((approval) => <Approval key={approval.approval_id} api={api} item={approval} changed={() => { setRevision((v) => v + 1); void refresh(); }} />)}<RunPendingControls api={api} view={pendingView} changed={() => { setRevision((v) => v + 1); void refresh(); }} /></>}
        </section>)}
      </div>
      {active && detailsOpen && <ToolArtifacts api={api} session={summary?.session ?? null} run={summary?.run === selected ? selected : null} revision={events.events.filter((event) => ['ActionCompleted', 'ActionOutcomeUnknown', 'ActionFailed', 'ActionCancelled'].includes(event.kind)).length} />}
      <SessionArtifacts api={api} session={savedSession()} revision={artifactRevision} />
      {pending === 'upload' && <p className="notice" role="status">正在核实附件：{commands.pendingFilename}<button type="button" className="secondary-button" disabled={busy || !runtime} onClick={() => void upload()}>核实原上传</button></p>}
      <form className="composer" onSubmit={submit}>
        <label className="sr-only" htmlFor="message">给 forme 的消息</label>
        <textarea id="message" value={draft} onChange={(e) => setDraft(e.target.value)} rows={3} maxLength={6000} disabled={locked || currentSession?.archived} placeholder="说说你的想法，或交给 forme 一件事…" required />
        <div className="composer-footer"><span className="muted">{pending ? '保留原操作标识，核实后再开始下一项。' : '任务执行与审批始终由主脑管理。'}</span>
          <label className="secondary-button attachment-button">{busy && pending === 'upload' ? '正在上传…' : '添加附件'}<input type="file" onChange={chooseFile} disabled={locked || !runtime || currentSession?.archived} /></label>
          <button className="primary-button" disabled={busy || pending === 'upload' || sessionActionsLocked || workspaceBusy || workspaceChange !== null || !runtime || currentSession?.archived || (!pending && !draft.trim())}>{busy ? '正在提交…' : pending === 'turn' ? '核实原提交' : '发送'}</button></div>
      </form>
    </div>{detailsOpen && <aside className="details-panel" aria-label="任务详情">
      <button className="secondary-button" onClick={() => setDetailsOpen(false)}>收起详情</button><h2>任务详情</h2><p className="status-pill">{summary ? statusLabel(summary.status) : '正在读取详情'}</p>
      <dl><div><dt>实际运行工作空间</dt><dd>{summary?.workspace ?? '尚无可显示的运行绑定'}</dd></div><div><dt>事件进度</dt><dd>{events.watermark}</dd></div><div><dt>验证与结果</dt><dd>{summary?.result ? '已记录运行结果，请查看详情' : '尚未确认'}</dd></div></dl>
      {summary && !isFinished(summary.status) && <button className="secondary-button" onClick={() => void cancel()}>请求停止任务</button>}
      <p className="muted small">停止请求不代表动作已经停止；以服务器终态为准。</p>
      {events.events.map((event) => <details className="event-row" key={event.event_id}><summary>#{event.stream_seq} · {event.kind}</summary><pre>{JSON.stringify(event.payload, null, 2)}</pre></details>)}
    </aside>}</div>
  </section>;
}

export function statusLabel(status: RunSummary['status']): string {
  return { Accepted: '已接收', Running: '处理中', Waiting: '等待处理', Complete: '运行已完成', Aborted: '已停止', Failed: '运行失败', Limited: '达到运行边界' }[status];
}
