import { useState } from 'react';
import type { FormEvent } from 'react';
import { newKey } from '../api/client.ts';
import type { WorkbenchClient } from '../api/client.ts';
import type { SessionResponse } from '../types.ts';
import { candidateLabel } from '../bootstrap.ts';
import type { BootstrapCandidate } from '../bootstrap.ts';

interface Props { api: WorkbenchClient; unclaimed: boolean; authenticated: (view: SessionResponse) => void }

export function Entry({ api, unclaimed, authenticated }: Props) {
  const [operation] = useState(() => {
    const key = sessionStorage.getItem('forme.bootstrap.operation') ?? newKey('claim');
    sessionStorage.setItem('forme.bootstrap.operation', key); return key;
  });
  const [candidates, setCandidates] = useState<BootstrapCandidate[]>([]);
  const [choice, setChoice] = useState('');
  const [selected, setSelected] = useState(false);
  const candidate = candidates.find(item => item.candidate_ref === choice);
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
        const view = await api.bootstrapCandidates();
        if (view.operation !== operation) throw new Error('初始化操作已变化，请核实原操作。');
        setCandidates(view.candidates); setChoice(view.selected ?? (view.candidates.length === 1 ? view.candidates[0].candidate_ref : ''));
        setSelected(view.selected !== null); setProved(true); form.reset();
      } else if (unclaimed) {
        if (candidate?.status !== 'available') throw new Error('请先选择已通过验证的数据。');
        const password = String(data.get('password') ?? '');
        if (password !== data.get('confirm')) throw new Error('两次输入的密码不一致。');
        if (!selected) {
          if (candidate.kind === 'new_brain_target') await api.select(candidate.candidate_ref, operation);
          else await api.selectLegacy(candidate, operation);
          setSelected(true);
        }
        const view = candidate.kind === 'new_brain_target'
          ? await api.claim(candidate.candidate_ref, String(data.get('username')), password, operation)
          : await api.claimLegacy(candidate.candidate_ref, String(data.get('username')), password, operation);
        sessionStorage.removeItem('forme.bootstrap.operation'); authenticated(view);
        form.reset();
      } else {
        authenticated(await api.login(String(data.get('username')), String(data.get('password'))));
        form.reset();
      }
    } catch (problem) { setError(problem instanceof Error ? problem.message : '请求未完成。'); }
    finally { setBusy(false); }
  }

  return <main className="entry" id="main-content">
    <section className="entry-intro"><p className="eyebrow">FOR ME / WORKBENCH</p>
      <h1>把复杂的事，<br /><span>持续推进下去。</span></h1>
      <p>一个会记住上下文、理解你的工作方式，并能在后台持续执行任务的工作台。</p>
      <div className="entry-principles"><span><b>01</b>主动思维</span><span><b>02</b>长期记忆</span><span><b>03</b>受控执行</span></div>
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
        {proved && <fieldset disabled={busy || selected}><legend>选择本次使用的数据</legend>
          {candidates.map(item => <label key={item.candidate_ref}><input type="radio" name="data-candidate" value={item.candidate_ref}
            checked={choice === item.candidate_ref} disabled={item.status !== 'available'} onChange={() => setChoice(item.candidate_ref)} />
            {candidateLabel(item)}{item.status !== 'available' && ' · 尚未通过验证'}</label>)}
        </fieldset>}
        {candidate && <p className="notice">{candidate.status === 'available'
          ? candidate.kind === 'new_brain_target' ? '将创建新的主脑。' : '原数据将保留；账号仅在数据恢复与激活完成后创建，随后再配置模型。'
          : '数据验证尚未通过，请检查本机维护状态。'}</p>}
        {error && <p className="form-error" role="alert">{error}</p>}
        <button className="primary-button" disabled={busy || (proved && candidate?.status !== 'available')}>{busy ? '正在确认…' : unclaimed ? proved ? '完成认领' : '验证本机证明' : '登录'}</button>
      </form>
      <p className="entry-footnote">登录即进入这台 forme 的完整控制面。工作台权限与 Agent 的执行治理分别受控。</p>
    </section>
  </main>;
}
