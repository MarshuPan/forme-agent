import { useEffect, useRef, useState } from 'react';
import type { ChangeEvent, FormEvent } from 'react';
import { ApiError, newKey } from '../api/client.ts';
import type { WorkbenchClient } from '../api/client.ts';
import { emptyEvents, parseEvents, readDelay, reduceEvents } from '../events.ts';
import type { EventState } from '../events.ts';
import { isFinished } from '../types.ts';
import type { PendingApproval, RunPendingView, RunSummary, RuntimeView, WorkbenchSessionRecord } from '../types.ts';
import { Approval } from './Approval.tsx';
import { ConversationCommands } from '../conversation.ts';
import { SessionArtifacts } from './SessionArtifacts.tsx';
import { RunPendingControls } from './RunPendingControls.tsx';
import { runStatusMessage } from '../run-status.ts';
import { acceptedUserInput } from '../conversation-view.ts';
import { SessionActions } from './SessionActions.tsx';

interface Props {
  api: WorkbenchClient; runtime: RuntimeView | null; runs: RunSummary[];
  sessions: WorkbenchSessionRecord[]; selected: string | null; select: (run: string | null) => void;
  refresh: () => Promise<void>; refreshSessions: () => Promise<void>;
  observed: (run: RunSummary) => void;
  workspaceRevision: number;
  onPendingChange: (pending: boolean) => void;
}

type WorkspaceChange = { session: string; expectedUpdatedAt: number; workspace: string | null; key: string };

export function Conversation({ api, runtime, runs, sessions, selected, select, refresh, refreshSessions, observed, workspaceRevision, onPendingChange }: Props) {
  const [draft, setDraft] = useState('');
  const [session, setSession] = useState(() => sessions[0]?.session ?? newKey('session'));
  const [sessionTitle, setSessionTitle] = useState('新对话');
  const [commands] = useState(() => new ConversationCommands());
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
  const [summary, setSummary] = useState<RunSummary | null>(null);
  const [answer, setAnswer] = useState<string | null>(null);
  const [userInput, setUserInput] = useState<string | null>(null);
  const [events, setEvents] = useState<EventState>(() => emptyEvents(''));
  const [approvals, setApprovals] = useState<PendingApproval[]>([]);
  const [pendingView, setPendingView] = useState<RunPendingView | null>(null);
  const [error, setError] = useState('');
  const [revision, setRevision] = useState(0);
  const cancelOperation = useRef(newKey('cancel'));
  const alive = useRef(true);

  useEffect(() => { alive.current = true; return () => { alive.current = false; commands.dispose(); }; }, [commands]);
  useEffect(() => { onPendingChange(pending !== null || sessionActionsLocked); }, [pending, sessionActionsLocked, onPendingChange]);

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
    if (!selected) { setSummary(null); setEvents(emptyEvents('')); setAnswer(null); setUserInput(null); setApprovals([]); setPendingView(null); return; }
    let closed = false, timer: ReturnType<typeof setTimeout> | undefined, attempt = 0;
    let snapshot = emptyEvents(selected);
    setSummary(null); setAnswer(null); setUserInput(null); setEvents(snapshot); setApprovals([]); setPendingView(null); setError('');
    async function poll() {
      try {
        const current = await api.run(selected!);
        const incoming = parseEvents(await api.events(selected!, snapshot.watermark));
        let next = reduceEvents(snapshot, incoming);
        if (next.gap) next = reduceEvents(emptyEvents(selected!), parseEvents(await api.events(selected!, 0)));
        if (next.gap) throw new Error('事件暂不连续，正在等待重新同步。');
        const input = acceptedUserInput(next, current.session);
        const [output, waiting, pendingState] = await Promise.all([api.answer(selected!), api.approvals(current.session), api.runPending(selected!)]);
        if (closed) return;
        snapshot = next; attempt = 0; setEvents(snapshot); setSummary(current); setSession(current.session);
        setAnswer(output.answer); setUserInput(input); observed(current); setApprovals(waiting.filter((item) => item.run === selected)); setPendingView(pendingState); setError('');
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
  }, [api, selected, revision, observed]);

  async function submit(event: FormEvent) {
    event.preventDefault(); if (!runtime || currentSession?.archived || sessionActionsLocked || workspaceBusy || workspaceChange !== null) return;
    const request = commands.submit(api, savedSession(), draft, runtime.agent_profile);
    if (!request) return;
    commandState(); setError('');
    try {
      const result = await request;
      if (!alive.current) return;
      setSession(result.session); setDraft(''); select(result.run);
      await Promise.all([refresh(), refreshSessions()]);
    } catch (problem) { if (alive.current) setError(problem instanceof Error ? problem.message : '提交结果尚未确认。'); }
    finally { if (alive.current) commandState(); }
  }
  function savedSession() { return sessions.some((item) => item.session === session) ? session : null; }
  function commandState() { redraw((value) => value + 1); }
  async function upload(file?: File) {
    const request = commands.uploadFile(api, savedSession(), file);
    if (!request) return;
    commandState(); setError('');
    try {
      const result = await request;
      if (!alive.current) return;
      setSession(result.session); setArtifactRevision((value) => value + 1); await refreshSessions();
    } catch (problem) { if (alive.current) setError(problem instanceof Error ? problem.message : '附件上传结果尚未确认。'); }
    finally { if (alive.current) commandState(); }
  }
  function chooseFile(event: ChangeEvent<HTMLInputElement>) {
    const file = event.target.files?.[0]; event.target.value = '';
    if (file) void upload(file);
  }
  async function sessionUpdated(updated: WorkbenchSessionRecord) {
    if (updated.archived) {
      setSession(sessions.find((item) => !item.archived && item.session !== updated.session)?.session ?? newKey('session'));
      select(null);
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
  async function chooseSession(value: string) {
    if (!value || locked) return;
    setSession(value); select(runs.filter((run) => run.session === value).at(-1)?.run ?? null);
  }
  async function cancel() {
    if (!selected) return;
    try { await api.cancel(selected, cancelOperation.current); setRevision((v) => v + 1); }
    catch (problem) { setError(problem instanceof Error ? problem.message : '停止请求尚未确认。'); }
  }
  const related = runs.filter((run) => run.session === session);
  const currentSession = sessions.find((item) => item.session === session) ?? null;
  const selectableWorkspaces = [...new Set([...workspaceOptions, ...(currentSession?.workspace ? [currentSession.workspace] : [])])];
  return <section className="conversation" aria-labelledby="conversation-title">
    <header className="view-header"><div><p className="eyebrow">与你一起推进</p><h1 id="conversation-title">对话</h1></div>
      <div className="button-row"><select aria-label="当前会话" disabled={locked} value={savedSession() ?? ''} onChange={(event) => void chooseSession(event.target.value)}><option value="">新会话（发送时创建）</option>{sessions.filter((item) => !item.archived).map((item) => <option value={item.session} key={item.session}>{item.title}</option>)}</select><button className="secondary-button" disabled={locked} onClick={() => { select(null); setSession(newKey('session')); setSessionTitle('新对话'); setSummary(null); setAnswer(null); setEvents(emptyEvents('')); }}>新对话</button>
      </div>
    </header>
    {currentSession && <SessionActions key={currentSession.session} api={api} session={currentSession} locked={commandLocked} lock={setSessionActionsLocked} refresh={refreshSessions} confirmed={sessionUpdated} />}
    {currentSession && <div className="session-context-row"><label className="session-context-control">后续运行工作空间<select aria-label="后续运行工作空间" value={currentSession.workspace ?? ''} disabled={locked || workspaceOptions.length === 0} onChange={(event) => associateWorkspace(event.target.value)}><option value="">默认工作空间{runtime ? `（${runtime.workspace}）` : ''}</option>{selectableWorkspaces.map((workspace) => <option key={workspace} value={workspace}>{workspace === runtime?.workspace ? `${workspace}（默认）` : workspace}</option>)}</select></label>{workspaceChange?.session === session && <button className="secondary-button" disabled={workspaceBusy} onClick={verifyWorkspaceChange}>{workspaceBusy ? '正在核实…' : '核实原关联'}</button>}{workspaceOptionsError && <span className="muted small" role="status">{workspaceOptionsError}</span>}</div>}
    {!runtime && <p className="notice" role="status">主脑暂不可用。可以在设置中查看连接状态。</p>}
    {currentSession?.archived && <p className="notice" role="status">当前会话已归档，可以查看历史记录；请新建对话以继续交流。</p>}
    {error && <p role="alert" className="form-error">{error}</p>}
    <div className="conversation-layout"><div className="conversation-main">
      {related.length > 1 && <nav className="run-tabs" aria-label="当前会话运行">{related.map((run, i) => <button disabled={locked} className={selected === run.run ? 'active' : ''} key={run.run} onClick={() => select(run.run)}>第 {i + 1} 次 · {statusLabel(run.status)}</button>)}</nav>}
      <div className="messages" aria-live="polite" aria-relevant="additions text">
        {!summary && !selected && <div className="empty-state"><h2>{sessionTitle}，想一起做点什么？</h2><p>从一个问题、一项任务，或一个长期项目开始。</p></div>}
        {userInput !== null && <article className="answer user-message"><p className="message-label">你</p><div className="message-text">{userInput}</div></article>}
        {answer !== null && <article className="answer"><p className="message-label">forme</p><div className="message-text">{answer}</div></article>}
        {summary && answer === null && <p className="muted">{runStatusMessage(summary)}</p>}
        {approvals.map((item) => <Approval key={item.approval_id} api={api} item={item} changed={() => { setRevision((v) => v + 1); void refresh(); }} />)}
        <RunPendingControls api={api} view={pendingView} changed={() => { setRevision((v) => v + 1); void refresh(); }} />
      </div>
      <SessionArtifacts api={api} session={savedSession()} revision={artifactRevision} />
      {pending === 'upload' && <p className="notice" role="status">正在核实附件：{commands.pendingFilename}<button type="button" className="secondary-button" disabled={busy || !runtime} onClick={() => void upload()}>核实原上传</button></p>}
      <form className="composer" onSubmit={submit}>
        <label className="sr-only" htmlFor="message">给 forme 的消息</label>
        <textarea id="message" value={draft} onChange={(e) => setDraft(e.target.value)} rows={3} maxLength={6000} disabled={locked || currentSession?.archived} placeholder="说说你的想法，或交给 forme 一件事…" required />
        <div className="composer-footer"><span className="muted">{pending ? '保留原操作标识，核实后再开始下一项。' : '任务执行与审批始终由主脑管理。'}</span>
          <label className="secondary-button attachment-button">{busy && pending === 'upload' ? '正在上传…' : '添加附件'}<input type="file" onChange={chooseFile} disabled={locked || !runtime || currentSession?.archived} /></label>
          <button className="primary-button" disabled={busy || pending === 'upload' || sessionActionsLocked || workspaceBusy || workspaceChange !== null || !runtime || currentSession?.archived || (!pending && !draft.trim())}>{busy ? '正在提交…' : pending === 'turn' ? '核实原提交' : '发送'}</button></div>
      </form>
    </div><aside className="details-panel" aria-label="任务详情">
      <h2>任务详情</h2><p className="status-pill">{summary ? statusLabel(summary.status) : '尚未选择运行'}</p>
      <dl><div><dt>实际运行工作空间</dt><dd>{summary?.workspace ?? '尚无可显示的运行绑定'}</dd></div><div><dt>事件进度</dt><dd>{events.watermark}</dd></div><div><dt>验证与结果</dt><dd>{summary?.result ? '已记录运行结果，请查看详情' : '尚未确认'}</dd></div></dl>
      {summary && !isFinished(summary.status) && <button className="secondary-button" onClick={() => void cancel()}>请求停止任务</button>}
      <p className="muted small">停止请求不代表动作已经停止；以服务器终态为准。</p>
      {events.events.map((event) => <details className="event-row" key={event.event_id}><summary>#{event.stream_seq} · {event.kind}</summary><pre>{JSON.stringify(event.payload, null, 2)}</pre></details>)}
    </aside></div>
  </section>;
}

export function statusLabel(status: RunSummary['status']): string {
  return { Accepted: '已接收', Running: '处理中', Waiting: '等待处理', Complete: '运行已完成', Aborted: '已停止', Failed: '运行失败', Limited: '达到运行边界' }[status];
}
