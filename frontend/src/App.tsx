import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { ApiError, WorkbenchClient } from './api/client.ts';
import type { HostStatus, RunSummary, RuntimeView, SessionResponse, SessionView, WorkbenchSessionRecord } from './types.ts';
import { Entry } from './components/Entry.tsx';
import { Conversation, statusLabel } from './components/Conversation.tsx';
import { Settings } from './components/Views.tsx';
import { BrainInbox, CognitionView } from './components/BrainViews.tsx';
import { WorkspaceView } from './components/Workspace.tsx';
import { mergeObservedRun } from './conversation-view.ts';
import type { MaterialLifecycleTarget } from './memory-material.ts';
import { createAuthInvalidationSync } from './auth-sync.ts';
import { createSessionVisibilityGuard } from './session-visibility.ts';
import { CoreReadinessCancelled, waitForCoreReady } from './core-readiness.ts';

const tabs = [
  ['conversation', '工作台', '⌁', '把任务交给 forme，持续推进'],
  ['workspace', '工作空间', '▦', '项目、目标与长期任务'],
  ['inbox', '收件箱', '○', '审批、建议与需要你决定的事项'],
  ['cognition', '记忆与认知', '◌', '查看 forme 如何理解你和正在形成的判断'],
  ['settings', '设置', '⚙', '模型、渠道、能力与产品维护'],
] as const;
type Tab = typeof tabs[number][0];

export function App() {
  const [entry, setEntry] = useState<'checking' | 'unclaimed' | 'login' | 'ready' | 'unavailable'>('checking');
  const [session, setSession] = useState<SessionView | null>(null);
  const [runtime, setRuntime] = useState<RuntimeView | null>(null);
  const [productStatus, setProductStatus] = useState<HostStatus | null>(null);
  const [coreConnecting, setCoreConnecting] = useState(false);
  const [runs, setRuns] = useState<RunSummary[]>([]);
  const [sessions, setSessions] = useState<WorkbenchSessionRecord[]>([]);
  const [selected, setSelected] = useState<string | null>(null);
  const [conversationPending, setConversationPending] = useState(false);
  const [loggingOut, setLoggingOut] = useState(false);
  const [workspaceRevision, setWorkspaceRevision] = useState(0);
  const [tab, setTab] = useState<Tab>('conversation');
  const [menu, setMenu] = useState(false);
  const mobileMenuButton = useRef<HTMLButtonElement>(null);
  const firstMobileNavItem = useRef<HTMLButtonElement>(null);
  const wasMenuOpen = useRef(false);
  const [issue, setIssue] = useState('');
  const [materialTarget, setMaterialTarget] = useState<MaterialLifecycleTarget | null>(null);
  const authSync = useRef<ReturnType<typeof createAuthInvalidationSync> | null>(null);
  const coreProbe = useRef<AbortController | null>(null);
  const consumeMaterialTarget = useCallback(() => setMaterialTarget(null), []);
  const clear = useCallback((notifyPeers = true) => {
    coreProbe.current?.abort();
    coreProbe.current = null;
    if (notifyPeers) authSync.current?.notifyInvalidated();
    setSession(null); setRuntime(null); setProductStatus(null); setCoreConnecting(false); setRuns([]); setSessions([]); setSelected(null); setMaterialTarget(null); setConversationPending(false); setMenu(false); setEntry('login');
  }, []);
  const api = useMemo(() => new WorkbenchClient(fetch, clear), [clear]);
  const refreshRuns = useCallback(async () => { const items = await api.runs(); setRuns((current) => items.map((run) => current.find((item) => item.run === run.run && item.last_stream_seq > run.last_stream_seq) ?? run)); }, [api]);
  const observeRun = useCallback((run: RunSummary) => { setRuns((items) => mergeObservedRun(items, run)); }, []);
  const refreshSessions = useCallback(async () => { const view = await api.workbenchSessions(); setSessions(view.sessions); }, [api]);
  const refreshCore = useCallback(async () => {
    coreProbe.current?.abort();
    const controller = new AbortController();
    coreProbe.current = controller;
    setCoreConnecting(true);
    try {
      try {
        const status = await api.productStatus();
        if (!controller.signal.aborted) setProductStatus(status);
      } catch (problem) {
        if (!controller.signal.aborted) setIssue(problem instanceof Error ? problem.message : '产品状态暂不可用。');
      }
      const next = await waitForCoreReady(() => api.runtime(), { signal: controller.signal });
      if (controller.signal.aborted) return;
      setRuntime(next); setIssue('');
      try { setProductStatus(await api.productStatus()); }
      catch (problem) { if (!controller.signal.aborted) setIssue(problem instanceof Error ? problem.message : '产品状态暂不可用。'); }
      try { await Promise.all([refreshSessions(), refreshRuns()]); }
      catch (problem) { if (!controller.signal.aborted) setIssue(problem instanceof Error ? problem.message : '工作台历史暂不可用。'); }
    } catch (problem) {
      if (controller.signal.aborted || problem instanceof CoreReadinessCancelled) return;
      setRuntime(null); setIssue(problem instanceof Error ? problem.message : '主脑服务暂不可用。');
    } finally {
      if (coreProbe.current === controller) { coreProbe.current = null; setCoreConnecting(false); }
    }
  }, [api, refreshRuns, refreshSessions]);
  function authenticated(view: SessionResponse) { setSession(view.session); setEntry('ready'); }

  useEffect(() => {
    if (menu) {
      firstMobileNavItem.current?.focus();
    } else if (wasMenuOpen.current) {
      const trigger = mobileMenuButton.current;
      if (trigger?.getClientRects().length) trigger.focus();
      else document.getElementById('main-content')?.focus();
    }
    wasMenuOpen.current = menu;
  }, [menu]);

  useEffect(() => {
    if (!menu) return;
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key !== 'Escape') return;
      event.preventDefault();
      setMenu(false);
    };
    window.addEventListener('keydown', closeOnEscape);
    return () => window.removeEventListener('keydown', closeOnEscape);
  }, [menu]);

  useEffect(() => {
    const sync = createAuthInvalidationSync(() => {
      api.clear();
      clear(false);
    });
    authSync.current = sync;
    return () => {
      sync.close();
      if (authSync.current === sync) authSync.current = null;
    };
  }, [api, clear]);

  useEffect(() => {
    let closed = false;
    async function check() {
      try {
        const bootstrap = await api.bootstrap(); if (closed) return;
        if (!bootstrap.claimed) { setEntry('unclaimed'); return; }
        const view = await api.session(); if (!closed) authenticated(view);
      } catch (problem) {
        if (closed) return;
        setEntry(problem instanceof ApiError && problem.status === 401 ? 'login' : 'unavailable'); setIssue(problem instanceof Error ? problem.message : '服务暂不可用。');
      }
    }
    void check(); return () => { closed = true; };
  }, [api]);
  useEffect(() => {
    if (entry !== 'ready') return;
    void refreshCore();
    return () => { coreProbe.current?.abort(); };
  }, [entry, refreshCore]);
  useEffect(() => {
    if (entry !== 'ready') return;
    const guard = createSessionVisibilityGuard({
      verify: () => api.session(), hidden: () => document.hidden,
      conceal: () => document.documentElement.classList.add('privacy-hidden'),
      reveal: () => document.documentElement.classList.remove('privacy-hidden'),
      invalidated: () => { api.clear(); clear(false); },
    });
    const hide = () => guard.hide();
    const show = () => { void guard.show(); };
    const visibility = () => { if (document.hidden) hide(); else show(); };
    const timer = setInterval(() => void guard.check(), 60_000);
    window.addEventListener('pagehide', hide); window.addEventListener('pageshow', show);
    document.addEventListener('visibilitychange', visibility);
    return () => { clearInterval(timer); window.removeEventListener('pagehide', hide); window.removeEventListener('pageshow', show); document.removeEventListener('visibilitychange', visibility); guard.close(); };
  }, [api, clear, entry]);

  async function logout() {
    setLoggingOut(true);
    try { await api.logout(); clear(); }
    catch (problem) { setIssue(problem instanceof Error ? problem.message : '退出结果未确认。'); }
    finally { setLoggingOut(false); }
  }
  function open(run: string) { if (!conversationPending) setSelected(run); setTab('conversation'); setMenu(false); }
  const serviceLabel = runtime ? '主脑已连接' : coreConnecting ? '主脑启动中' : productStatus ? productStatusLabel(productStatus) : '已登录 · 主脑离线';

  return <><a href="#main-content" className="skip-link">跳到主要内容</a><header className="topbar">
    <a className="brand" href="/" aria-label="forme 首页"><span className="brand-mark">f</span><span className="brand-word">forme</span></a>
    <div className="topbar-context"><span className="context-label">当前工作区</span><strong>{runtime?.workspace ?? '本机工作区'}</strong></div>
    <div className="topbar-actions">
      <span className="service-state" title={serviceLabel}><i data-ready={Boolean(runtime)} />{entry === 'ready' ? serviceLabel : entry === 'checking' ? '连接本机服务' : '持续任务工作台'}</span>
      {entry === 'ready' && <><button ref={mobileMenuButton} className="mobile-menu icon-button" aria-expanded={menu} aria-controls="main-navigation" aria-label={menu ? '关闭导航' : '打开导航'} onClick={() => setMenu((v) => !v)}>{menu ? '×' : '☰'}</button><button className="account-button" aria-label="退出登录" title="退出登录" disabled={loggingOut} onClick={() => void logout()}><span className="avatar">{session?.login_name.slice(0, 1).toUpperCase() ?? 'F'}</span><span className="account-name">{session?.login_name}</span></button></>}
    </div>
  </header>
    {['unclaimed', 'login'].includes(entry) && <Entry key={entry} api={api} unclaimed={entry === 'unclaimed'} authenticated={authenticated} />}
    {entry === 'checking' && <main id="main-content" className="entry-status" role="status">正在确认本机服务与登录状态…</main>}
    {entry === 'unavailable' && <main id="main-content" className="entry-status"><h1>暂时无法连接</h1><p role="alert">{issue}</p><button className="primary-button" onClick={() => location.reload()}>重新检查</button></main>}
    {entry === 'ready' && session && <div className="app-shell"><aside className={`sidebar ${menu ? 'is-open' : ''}`} id="main-navigation">
      <div className="sidebar-intro"><span className="sidebar-kicker">FOR ME / WORKBENCH</span><p>持续推进复杂任务</p></div>
      <button className="new-task-button" onClick={() => { setTab('conversation'); setSelected(null); setWorkspaceRevision((value) => value + 1); setMenu(false); }}><span className="new-task-icon">＋</span><span>新任务</span><kbd>N</kbd></button>
      <nav aria-label="主导航" className="nav-groups">
        <div className="nav-group"><span className="nav-group-label">工作</span>{tabs.slice(0, 2).map(([key, label, icon, description], index) => <button key={key} title={description} ref={index === 0 ? firstMobileNavItem : undefined} className={`nav-item ${tab === key ? 'active' : ''}`} aria-current={tab === key ? 'page' : undefined} onClick={() => { setTab(key); if (key === 'conversation') setWorkspaceRevision((value) => value + 1); setMenu(false); }}><span className="nav-icon" aria-hidden="true">{icon}</span><span className="nav-label">{label}</span>{key === 'conversation' && runs.some((run) => !['Complete', 'Aborted', 'Failed'].includes(run.status)) && <span className="nav-badge">{runs.filter((run) => !['Complete', 'Aborted', 'Failed'].includes(run.status)).length}</span>}</button>)}</div>
        <div className="nav-group"><span className="nav-group-label">理解</span>{tabs.slice(2, 4).map(([key, label, icon, description]) => <button key={key} title={description} className={`nav-item ${tab === key ? 'active' : ''}`} aria-current={tab === key ? 'page' : undefined} onClick={() => { setTab(key); setMenu(false); }}><span className="nav-icon" aria-hidden="true">{icon}</span><span className="nav-label">{label}</span>{key === 'inbox' && runs.some((run) => run.status === 'Waiting') && <span className="nav-badge">!</span>}</button>)}</div>
        <div className="nav-group"><span className="nav-group-label">管理</span>{tabs.slice(4).map(([key, label, icon, description]) => <button key={key} title={description} className={`nav-item ${tab === key ? 'active' : ''}`} aria-current={tab === key ? 'page' : undefined} onClick={() => { setTab(key); setMenu(false); }}><span className="nav-icon" aria-hidden="true">{icon}</span><span className="nav-label">{label}</span></button>)}</div>
      </nav>
      <div className="recent-section"><div className="recent-heading"><span>最近任务</span><span className="count">{runs.length}</span></div><div className="recent-runs">{runs.slice(-8).reverse().map((run) => <button key={run.run} aria-pressed={selected === run.run} onClick={() => open(run.run)}><span>{run.session}</span><small><i className={`run-dot run-${run.status.toLowerCase()}`} />{statusLabel(run.status)}</small></button>)}{!runs.length && <p className="muted small">从新任务开始</p>}</div></div>
      <div className="sidebar-footer"><span className="service-state"><i data-ready={Boolean(runtime)} />{serviceLabel}</span><p className="sidebar-footnote">你的工作台持续在线</p></div>
    </aside><main id="main-content" className="main-content" tabIndex={-1} key={session.account_ref}>
      <div className="main-statusbar"><div><span className="breadcrumb-root">forme</span><span className="breadcrumb-separator">/</span><span>{tabs.find(([key]) => key === tab)?.[1]}</span></div><div className="main-status-actions"><span className="secure-label"><span className="secure-dot" />已连接本机主脑</span></div></div>
      <div hidden={tab !== 'conversation'}><Conversation active={tab === 'conversation' && !loggingOut} api={api} runtime={runtime} runs={runs} sessions={sessions} selected={selected} select={setSelected} refresh={refreshRuns} refreshSessions={refreshSessions} observed={observeRun} workspaceRevision={workspaceRevision} onPendingChange={setConversationPending} /></div>
      <div hidden={tab !== 'workspace'}><WorkspaceView api={api} runtime={runtime} open={open} /></div>
      {tab === 'inbox' && <BrainInbox api={api} open={open} />}
      {tab === 'cognition' && <CognitionView api={api} open={open} prepareForgetting={(target) => { setMaterialTarget(target); setTab('settings'); setMenu(false); }} />}
      {tab === 'settings' && <Settings api={api} runtime={runtime} issue={issue} refresh={() => void refreshCore()} signedOut={clear} materialTarget={materialTarget} consumeMaterialTarget={consumeMaterialTarget} />}
    </main></div>}
  </>;
}

function productStatusLabel(status: HostStatus): string {
  switch (status.core_health) {
    case 'unconfigured': return '主脑未配置';
    case 'starting': return '主脑启动中';
    case 'stopped': return '主脑已停止';
    case 'ready': return '主脑正在接入';
    case 'degraded': return '主脑受限';
    case 'unreachable': return '主脑不可达';
  }
}
