import { useCallback, useEffect, useMemo, useState } from 'react';
import { ApiError, WorkbenchClient } from './api/client.ts';
import type { RunSummary, RuntimeView, SessionResponse, SessionView, WorkbenchSessionRecord } from './types.ts';
import { Entry } from './components/Entry.tsx';
import { Conversation, statusLabel } from './components/Conversation.tsx';
import { Settings } from './components/Views.tsx';
import { BrainInbox, CognitionView } from './components/BrainViews.tsx';
import { WorkspaceView } from './components/Workspace.tsx';

const tabs = [ ['conversation', '对话'], ['workspace', '工作空间'], ['inbox', '收件箱'], ['cognition', '记忆与认知'], ['settings', '设置'] ] as const;
type Tab = typeof tabs[number][0];

export function App() {
  const [entry, setEntry] = useState<'checking' | 'unclaimed' | 'login' | 'ready' | 'unavailable'>('checking');
  const [session, setSession] = useState<SessionView | null>(null);
  const [runtime, setRuntime] = useState<RuntimeView | null>(null);
  const [runs, setRuns] = useState<RunSummary[]>([]);
  const [sessions, setSessions] = useState<WorkbenchSessionRecord[]>([]);
  const [selected, setSelected] = useState<string | null>(null);
  const [conversationPending, setConversationPending] = useState(false);
  const [tab, setTab] = useState<Tab>('conversation');
  const [menu, setMenu] = useState(false);
  const [issue, setIssue] = useState('');
  const clear = useCallback(() => { setSession(null); setRuntime(null); setRuns([]); setSessions([]); setSelected(null); setConversationPending(false); setEntry('login'); }, []);
  const api = useMemo(() => new WorkbenchClient(fetch, clear), [clear]);
  const refreshRuns = useCallback(async () => { const items = await api.runs(); setRuns(items); }, [api]);
  const refreshSessions = useCallback(async () => { const view = await api.workbenchSessions(); setSessions(view.sessions); }, [api]);
  const refreshCore = useCallback(async () => {
    try { const [next] = await Promise.all([api.runtime(), refreshSessions()]); setRuntime(next); setIssue(''); await refreshRuns(); }
    catch (problem) { setRuntime(null); setTab('settings'); setIssue(problem instanceof Error ? problem.message : '主脑服务暂不可用。'); }
  }, [api, refreshRuns]);
  function authenticated(view: SessionResponse) { setSession(view.session); setEntry('ready'); }

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
  useEffect(() => { if (entry === 'ready') void refreshCore(); }, [entry, refreshCore]);
  useEffect(() => {
    if (entry !== 'ready') return;
    let checking = false;
    const verify = async () => { if (checking || document.hidden) return; checking = true; try { await api.session(); } catch { clear(); } finally { checking = false; } };
    // Revalidate before showing cached content restored by BFCache or another tab.
    const hide = () => { document.documentElement.classList.add('privacy-hidden'); };
    const show = () => { hide(); void verify().finally(() => document.documentElement.classList.remove('privacy-hidden')); };
    const visibility = () => { if (document.hidden) hide(); else show(); };
    const timer = setInterval(() => void verify(), 60_000);
    window.addEventListener('pagehide', hide); window.addEventListener('pageshow', show);
    document.addEventListener('visibilitychange', visibility);
    return () => { clearInterval(timer); window.removeEventListener('pagehide', hide); window.removeEventListener('pageshow', show); document.removeEventListener('visibilitychange', visibility); document.documentElement.classList.remove('privacy-hidden'); };
  }, [api, clear, entry]);

  async function logout() { try { await api.logout(); clear(); } catch (problem) { setIssue(problem instanceof Error ? problem.message : '退出结果未确认。'); } }
  function open(run: string) { if (!conversationPending) setSelected(run); setTab('conversation'); setMenu(false); }

  return <><a href="#main-content" className="skip-link">跳到主要内容</a><header className="topbar"><a className="brand" href="/" aria-label="forme 首页">forme<span className="brand-dot">.</span></a>
    <span className="service-state"><i data-ready={Boolean(runtime)} />{entry === 'ready' ? runtime ? '主脑已连接' : '已登录 · 主脑离线' : entry === 'checking' ? '连接本机服务' : '个人 Agent 工作台'}</span>
    {entry === 'ready' && <><button className="mobile-menu secondary-button" aria-expanded={menu} aria-controls="main-navigation" onClick={() => setMenu((v) => !v)}>导航</button><button className="text-button" onClick={() => void logout()}>退出登录</button></>}
  </header>
    {['unclaimed', 'login'].includes(entry) && <Entry key={entry} api={api} unclaimed={entry === 'unclaimed'} authenticated={authenticated} />}
    {entry === 'checking' && <main id="main-content" className="entry-status" role="status">正在确认本机服务与登录状态…</main>}
    {entry === 'unavailable' && <main id="main-content" className="entry-status"><h1>暂时无法连接</h1><p role="alert">{issue}</p><button className="primary-button" onClick={() => location.reload()}>重新检查</button></main>}
    {entry === 'ready' && session && <div className="app-shell"><aside className={`sidebar ${menu ? 'is-open' : ''}`} id="main-navigation"><p className="sidebar-owner">{session.login_name}<span>你的工作台</span></p>
      <nav aria-label="主导航">{tabs.map(([key, label]) => <button key={key} className={`nav-item ${tab === key ? 'active' : ''}`} aria-current={tab === key ? 'page' : undefined} onClick={() => { setTab(key); setMenu(false); }}>{label}<span aria-hidden="true">↗</span></button>)}</nav>
      <p className="section-label">最近运行</p><div className="recent-runs">{runs.slice(-12).reverse().map((run) => <button key={run.run} aria-pressed={selected === run.run} onClick={() => open(run.run)}><span>{run.session}</span><small>{statusLabel(run.status)}</small></button>)}{!runs.length && <p className="muted small">还没有运行记录</p>}</div>
      <p className="sidebar-footnote">一个主脑，持续协同。</p>
    </aside><main id="main-content" className="main-content" key={session.account_ref}>
      <div hidden={tab !== 'conversation'}><Conversation api={api} runtime={runtime} runs={runs} sessions={sessions} selected={selected} select={setSelected} refresh={refreshRuns} refreshSessions={refreshSessions} onPendingChange={setConversationPending} /></div>
      {tab === 'workspace' && <WorkspaceView api={api} runtime={runtime} />}
      {tab === 'inbox' && <BrainInbox api={api} runs={runs} open={open} />}
      {tab === 'cognition' && <CognitionView api={api} open={open} />}
      {tab === 'settings' && <Settings api={api} runtime={runtime} issue={issue} refresh={() => void refreshCore()} signedOut={clear} />}
    </main></div>}
  </>;
}
