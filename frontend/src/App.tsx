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

const tabs = [ ['conversation', '对话'], ['workspace', '工作空间'], ['inbox', '收件箱'], ['cognition', '记忆与认知'], ['settings', '设置'] ] as const;
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

  async function logout() { try { await api.logout(); clear(); } catch (problem) { setIssue(problem instanceof Error ? problem.message : '退出结果未确认。'); } }
  function open(run: string) { if (!conversationPending) setSelected(run); setTab('conversation'); setMenu(false); }
  const serviceLabel = runtime ? '主脑已连接' : coreConnecting ? '主脑启动中' : productStatus ? productStatusLabel(productStatus) : '已登录 · 主脑离线';

  return <><a href="#main-content" className="skip-link">跳到主要内容</a><header className="topbar"><a className="brand" href="/" aria-label="forme 首页">forme<span className="brand-dot">.</span></a>
    <span className="service-state"><i data-ready={Boolean(runtime)} />{entry === 'ready' ? serviceLabel : entry === 'checking' ? '连接本机服务' : '个人 Agent 工作台'}</span>
    {entry === 'ready' && <><button ref={mobileMenuButton} className="mobile-menu secondary-button" aria-expanded={menu} aria-controls="main-navigation" onClick={() => setMenu((v) => !v)}>{menu ? '关闭导航' : '导航'}</button><button className="text-button" onClick={() => void logout()}>退出登录</button></>}
  </header>
    {['unclaimed', 'login'].includes(entry) && <Entry key={entry} api={api} unclaimed={entry === 'unclaimed'} authenticated={authenticated} />}
    {entry === 'checking' && <main id="main-content" className="entry-status" role="status">正在确认本机服务与登录状态…</main>}
    {entry === 'unavailable' && <main id="main-content" className="entry-status"><h1>暂时无法连接</h1><p role="alert">{issue}</p><button className="primary-button" onClick={() => location.reload()}>重新检查</button></main>}
    {entry === 'ready' && session && <div className="app-shell"><aside className={`sidebar ${menu ? 'is-open' : ''}`} id="main-navigation"><p className="sidebar-owner">{session.login_name}<span>SuperAdmin · 你的工作台</span></p>
      <nav aria-label="主导航">{tabs.map(([key, label], index) => <button key={key} ref={index === 0 ? firstMobileNavItem : undefined} className={`nav-item ${tab === key ? 'active' : ''}`} aria-current={tab === key ? 'page' : undefined} onClick={() => { setTab(key); if (key === 'conversation') setWorkspaceRevision((value) => value + 1); setMenu(false); }}>{label}<span aria-hidden="true">↗</span></button>)}</nav>
      <p className="section-label">最近运行</p><div className="recent-runs">{runs.slice(-12).reverse().map((run) => <button key={run.run} aria-pressed={selected === run.run} onClick={() => open(run.run)}><span>{run.session}</span><small>{statusLabel(run.status)}</small></button>)}{!runs.length && <p className="muted small">还没有运行记录</p>}</div>
      <p className="sidebar-footnote">一个主脑，持续协同。</p>
    </aside><main id="main-content" className="main-content" tabIndex={-1} key={session.account_ref}>
      <div hidden={tab !== 'conversation'}><Conversation api={api} runtime={runtime} runs={runs} sessions={sessions} selected={selected} select={setSelected} refresh={refreshRuns} refreshSessions={refreshSessions} observed={observeRun} workspaceRevision={workspaceRevision} onPendingChange={setConversationPending} /></div>
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
