import { useState } from 'react';
import type { FormEvent } from 'react';
import { newKey } from '../api/client.ts';
import type { WorkbenchClient } from '../api/client.ts';
import type { DataCandidate, SessionResponse } from '../types.ts';

interface Props { api: WorkbenchClient; unclaimed: boolean; authenticated: (view: SessionResponse) => void }

export function Entry({ api, unclaimed, authenticated }: Props) {
  const [operation] = useState(() => newKey('claim'));
  const [candidate, setCandidate] = useState<DataCandidate | null>(null);
  const [proved, setProved] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault(); if (busy) return;
    const form = event.currentTarget, data = new FormData(form);
    setBusy(true); setError('');
    try {
      if (unclaimed && !proved) {
        await api.prove(String(data.get('proof') ?? ''), operation);
        const view = await api.data(); setCandidate(view); setProved(true); form.reset();
      } else if (unclaimed) {
        if (!candidate?.available) throw new Error('当前脑数据尚未验证，请先完成本机维护。');
        if (!candidate.selected) await api.select(candidate.candidate_ref, operation);
        const password = String(data.get('password') ?? '');
        if (password !== data.get('confirm')) throw new Error('两次输入的密码不一致。');
        authenticated(await api.claim(candidate.candidate_ref, String(data.get('username')), password, operation));
        form.reset();
      } else {
        authenticated(await api.login(String(data.get('username')), String(data.get('password'))));
        form.reset();
      }
    } catch (problem) { setError(problem instanceof Error ? problem.message : '请求未完成。'); }
    finally { setBusy(false); }
  }

  return <main className="entry" id="main-content">
    <section className="entry-intro"><p className="eyebrow">你的个人 Agent</p>
      <h1>让想法，<br />有个着落。</h1>
      <p>一个持续了解你、协同工作、积累经验的主脑。</p>
      <p className="entry-note">关闭这个页面，已授权的任务仍会继续。</p>
    </section>
    <section className="entry-panel" aria-labelledby="entry-title">
      <p className="eyebrow">{unclaimed ? '初始化 · 本机认领' : '欢迎回来'}</p>
      <h2 id="entry-title">{unclaimed ? proved ? '创建你的账号' : '连接你的 forme' : '进入工作台'}</h2>
      {unclaimed && !proved && <p className="muted">输入安装时生成的一次性证明，验证你对这台服务的控制权。</p>}
      <form onSubmit={submit} aria-busy={busy}>
        {unclaimed && !proved ? <label>本机一次性证明<input name="proof" type="password" autoComplete="off" required maxLength={128} /></label> : <>
          <label>账号<input name="username" autoComplete="username" required maxLength={64} autoFocus /></label>
          <label>密码<input name="password" type="password" autoComplete={unclaimed ? 'new-password' : 'current-password'} minLength={unclaimed ? 12 : 1} maxLength={128} required /></label>
          {unclaimed && <label>确认密码<input name="confirm" type="password" autoComplete="new-password" minLength={12} maxLength={128} required /></label>}
        </>}
        {candidate && <p className="notice">{candidate.available ? '脑数据已通过本机验证，可以继续认领。' : '数据验证尚未通过，请检查本机维护状态。'}</p>}
        {error && <p className="form-error" role="alert">{error}</p>}
        <button className="primary-button" disabled={busy || (proved && !candidate?.available)}>{busy ? '正在确认…' : unclaimed ? proved ? '完成认领' : '验证本机证明' : '登录'}</button>
      </form>
      <p className="entry-footnote">登录即进入这台 forme 的 SuperAdmin 控制面，拥有产品设置、项目、维护与数据管理权限；Agent 的具体执行仍受既定治理约束。</p>
    </section>
  </main>;
}
