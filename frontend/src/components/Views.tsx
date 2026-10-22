import { useEffect, useRef, useState } from 'react';
import type { FormEvent } from 'react';
import { ApiError, newKey } from '../api/client.ts';
import type { WorkbenchClient } from '../api/client.ts';
import { isRecord } from '../types.ts';
import type { ProductConfigView, ProductModelProfilePatch, RunSummary, RuntimeView } from '../types.ts';
import { statusLabel } from './Conversation.tsx';
import { draftFromConfig, emptyModel, patchFromDraft } from '../configuration.ts';
import type { ModelDraft } from '../configuration.ts';
import { ConfigurationActivation } from './ConfigurationActivation.tsx';

type Resource = { value: Record<string, unknown> | null; error: string; loading: boolean };
function useResource(load: () => Promise<unknown>, refresh: number): Resource {
  const [state, setState] = useState<Resource>({ value: null, error: '', loading: true });
  useEffect(() => {
    let active = true;
    setState({ value: null, error: '', loading: true });
    void load().then((value) => {
      if (!isRecord(value)) throw new Error('服务返回的工作台快照格式无效。');
      if (active) setState({ value, error: '', loading: false });
    })
      .catch((error: unknown) => { if (active) setState({ value: null, error: error instanceof Error ? error.message : '状态读取失败。', loading: false }); });
    return () => { active = false; };
  }, [load, refresh]);
  return state;
}

export function SnapshotView({ title, load, kind }: { title: string; load: () => Promise<unknown>; kind: 'workspace' | 'cognition' }) {
  const [revision, setRevision] = useState(0);
  const { value, error, loading } = useResource(load, revision);
  const projects = value && isRecord(value.projects) ? Object.values(value.projects) : [];
  const metrics = value && isRecord(value.metrics) ? value.metrics : {};
  return <section className="view" aria-labelledby={`${kind}-title`}><header className="view-header"><div><p className="eyebrow">{kind === 'workspace' ? '长期工作的上下文' : '从经验到判断'}</p><h1 id={`${kind}-title`}>{title}</h1></div><button className="secondary-button" onClick={() => setRevision((v) => v + 1)}>刷新</button></header>
    {loading && <p className="loading" role="status">正在读取主脑的最新状态…</p>}
    {error && <p className="form-error" role="alert">{error}</p>}
    {value && kind === 'workspace' && <div className="content-grid">{projects.length ? projects.map((project, index) => {
      const row = isRecord(project) ? project : {};
      return <article className="surface" key={String(row.reference ?? index)}><p className="eyebrow">{String(row.workspace ?? '')}</p><h2>{String(row.summary ?? row.reference ?? '项目')}</h2><dl><div><dt>目标</dt><dd>{String(row.goal ?? '未声明')}</dd></div><div><dt>监管周期</dt><dd>{typeof row.cadence_ms === 'number' ? `${row.cadence_ms / 1000} 秒` : '未声明'}</dd></div></dl><details><summary>项目约束与执行记录</summary><pre>{JSON.stringify(project, null, 2)}</pre></details></article>;
    }) : <div className="empty-state"><h2>还没有登记的长期项目</h2><p>在对话中和主脑明确目标、资料范围与完成标准后，再建立长期监管。</p></div>}</div>}
    {value && kind === 'cognition' && <>
      <p className="muted">原始记录与认知判断分层保存。以下统计来自主脑事件，不代表认知质量已经得到验证。</p>
      <div className="metric-grid">{[
        ['candidate_resolution', '候选处置'], ['verification', '结果验证'], ['ask_to_learn_usefulness', '问询的实际帮助'], ['workspace_supervision', '项目监管'],
      ].map(([key, label]) => {
        const metric = isRecord(metrics[key]) ? metrics[key] : {};
        return <article className="metric" key={key}><h2>{label}</h2><strong>{String(metric.numerator ?? 0)} <span>/ {String(metric.denominator ?? 0)}</span></strong><p>无法验证：{String(metric.unverifiable ?? 0)}</p></article>;
      })}</div><p className="notice">待处理候选：{String(metrics.candidate_backlog ?? 0)}。候选不会因为出现在页面上而自动成为稳定认知。</p>
      <details className="surface"><summary>统计范围与可追溯证据</summary><pre>{JSON.stringify(value, null, 2)}</pre></details>
    </>}
  </section>;
}

export function Inbox({ runs, open }: { runs: RunSummary[]; open: (run: string) => void }) {
  const pending = runs.filter((run) => ['Waiting', 'Failed', 'Limited'].includes(run.status));
  return <section className="view"><header className="view-header"><div><p className="eyebrow">需要你参与的事情</p><h1>收件箱</h1></div><span className="count">{pending.length}</span></header>
    <p className="muted">这里列出等待处理及失败的运行。查看不等于批准，运行详情保留原审批与未知结果。</p>
    {pending.length ? <div className="inbox-list">{pending.map((run) => <button key={run.run} className="inbox-item" onClick={() => open(run.run)}><span><strong>{statusLabel(run.status)}</strong><small>{run.session}</small></span><span>查看任务 →</span></button>)}</div> : <div className="empty-state"><h2>目前没有待处理的运行</h2><p>后台已授权的工作会继续，遇到需要你的决定时再回来。</p></div>}
  </section>;
}

export function Settings({ api, runtime, issue, refresh, signedOut }: { api: WorkbenchClient; runtime: RuntimeView | null; issue: string; refresh: () => void; signedOut: () => void }) {
  const [busy, setBusy] = useState(false), [error, setError] = useState('');
  const [config, setConfig] = useState<ProductConfigView | null>(null), [configError, setConfigError] = useState('');
  const [configLoading, setConfigLoading] = useState(true), [configBusy, setConfigBusy] = useState(false);
  const operation = useState(() => newKey('password'))[0];
  async function loadConfig() {
    setConfigLoading(true); setConfigError('');
    try { setConfig(await api.productConfig()); }
    catch (problem) { setConfigError(problem instanceof Error ? problem.message : '模型配置读取失败。'); }
    finally { setConfigLoading(false); }
  }
  useEffect(() => { void loadConfig(); }, [api]);
  async function changePassword(event: FormEvent<HTMLFormElement>) {
    event.preventDefault(); if (busy) return;
    const form = event.currentTarget, data = new FormData(form);
    setBusy(true); setError('');
    try {
      if (data.get('next') !== data.get('confirm')) throw new Error('两次输入的新密码不一致。');
      await api.password(String(data.get('current')), String(data.get('next')), operation); form.reset(); signedOut();
    } catch (problem) { setError(problem instanceof Error ? problem.message : '密码修改尚未确认。'); }
    finally { setBusy(false); }
  }
  return <section className="view"><header className="view-header"><div><p className="eyebrow">这台 forme 的运行情况</p><h1>设置与维护</h1></div><button className="secondary-button" onClick={refresh}>重新检查连接</button></header>
    <div className="settings-grid"><article className="surface"><h2>主脑服务</h2><p className="status-pill">{runtime ? '已连接正式运行时' : '主脑不可用'}</p><p className="muted">{issue || '页面与已授权后台任务分别运行。'}</p>
      {runtime && <><dl><div><dt>工作空间</dt><dd>{runtime.workspace}</dd></div><div><dt>可用模型配置</dt><dd>{runtime.model_profiles.join('、')}</dd></div></dl><ul className="service-list">{runtime.service_states.map((service) => <li key={service.name}><span>{service.name}</span><span>{service.status}</span></li>)}</ul></>}
      {!runtime && <p className="notice">账号服务仍可使用。请检查本机主脑服务和安装配置；这里不会另建一个空主脑。</p>}
    </article><article className="surface"><h2>模型配置</h2><p className="muted">密钥只写入本机受保护存储，不会回显到页面、浏览器存储或日志。</p>
      <ProductConfigEditor api={api} config={config} loading={configLoading} error={configError} busy={configBusy} onReload={() => void loadConfig()} onBusy={setConfigBusy} onError={setConfigError} onSaved={setConfig} />
      <ConfigurationActivation api={api} config={config} coreConnected={Boolean(runtime)} onSaved={setConfig} refresh={refresh} />
    </article><article className="surface"><h2>修改登录密码</h2><p className="muted">成功后撤销已有登录会话，脑数据与所有者身份保持不变。</p><form onSubmit={changePassword} aria-busy={busy}>
      <label>当前密码<input name="current" type="password" autoComplete="current-password" required maxLength={128} /></label>
      <label>新密码<input name="next" type="password" autoComplete="new-password" required minLength={12} maxLength={128} /></label>
      <label>确认新密码<input name="confirm" type="password" autoComplete="new-password" required minLength={12} maxLength={128} /></label>
      {error && <p role="alert" className="form-error">{error}</p>}<button className="primary-button" disabled={busy}>{busy ? '正在确认…' : '更新密码并重新登录'}</button>
    </form></article></div>
  </section>;
}

function ProductConfigEditor({ api, config, loading, error, busy, onReload, onBusy, onError, onSaved }: {
  api: WorkbenchClient; config: ProductConfigView | null; loading: boolean; error: string; busy: boolean;
  onReload: () => void; onBusy: (value: boolean) => void; onError: (value: string) => void;
  onSaved: (value: ProductConfigView) => void;
}) {
  const [models, setModels] = useState<ModelDraft[]>(() => draftFromConfig(config));
  const [loadedRevision, setLoadedRevision] = useState<number | null>(null);
  const pending = useRef<{ operation: string; revision: number; models: ProductModelProfilePatch[] } | null>(null);
  const [uncertain, setUncertain] = useState(false);
  useEffect(() => {
    if (config && config.active_revision !== loadedRevision) {
      setModels(draftFromConfig(config)); setLoadedRevision(config.active_revision);
    }
  }, [config, loadedRevision]);
  function update(index: number, change: Partial<ModelDraft>) {
    setModels((current) => current.map((model, position) => position === index ? { ...model, ...change } : model));
  }
  function makeDefault(index: number) {
    setModels((current) => current.map((model, position) => ({ ...model, default: position === index })));
  }
  async function save(event: FormEvent<HTMLFormElement>) {
    event.preventDefault(); if (busy || !config || !models.length) return;
    onBusy(true); onError('');
    try {
      pending.current ??= { operation: newKey('config'), revision: config.active_revision, models: patchFromDraft(models) };
      const { operation, revision, models: patch } = pending.current;
      const result = await api.saveProductConfig(revision, patch, operation);
      pending.current = null; setUncertain(false);
      onSaved(result.view); setLoadedRevision(result.view.active_revision); setModels(draftFromConfig(result.view));
    } catch (problem) {
      if (problem instanceof ApiError && (problem.status === 409 || (problem.status === 422 && problem.code === 'ConfigurationInvalid'))) { pending.current = null; setUncertain(false); }
      else setUncertain(true);
      onError(problem instanceof Error ? problem.message : '模型配置保存尚未确认。');
    }
    finally { onBusy(false); }
  }
  if (loading) return <p className="loading" role="status">正在读取模型配置…</p>;
  if (error && !config) return <><p className="form-error" role="alert">{error}</p><button className="secondary-button" type="button" onClick={onReload}>重新读取</button></>;
  return <form onSubmit={save} className="model-config-form" aria-busy={busy}>
    {config?.pending_restart && <p className="notice">配置版本 {config.active_revision} 已保存，等待主脑重新加载并确认。已确认版本：{config.acknowledged_revision ?? '无'}。</p>}
    {uncertain && <p className="notice">上次保存结果尚未确认，再次提交将核实同一份配置；请先完成核实再修改。</p>}
    {error && <p className="form-error" role="alert">{error}</p>}
    {models.map((model, index) => <fieldset className="model-profile" key={index} disabled={busy || uncertain}>
      <legend>模型 {index + 1}{model.default ? ' · 默认' : ''}</legend>
      <div className="form-grid model-form-grid">
        <label>配置标识<input value={model.id} onChange={(event) => update(index, { id: event.target.value })} required /></label>
        <label>Provider<input value={model.provider} onChange={(event) => update(index, { provider: event.target.value })} required /></label>
        <label>模型名<input value={model.model} onChange={(event) => update(index, { model: event.target.value })} required /></label>
        <label className="wide-field">兼容 API Base URL<input type="url" value={model.base_url} onChange={(event) => update(index, { base_url: event.target.value })} required /></label>
        <label className="wide-field">API Key<input type="password" value={model.credential} onChange={(event) => update(index, { credential: event.target.value })} placeholder={config?.models.find((saved) => saved.id === model.id)?.credential_configured ? '已配置，留空表示保持不变' : '只在保存时写入'} autoComplete="new-password" /></label>
        <label>上下文窗口<input type="number" min="1" value={model.context_window} onChange={(event) => update(index, { context_window: Number(event.target.value) })} required /></label>
        <label>能力强度<select value={model.strength} onChange={(event) => update(index, { strength: event.target.value as ModelDraft['strength'] })}><option value="basic">basic</option><option value="standard">standard</option><option value="strong">strong</option></select></label>
        <label>请求/分钟<input type="number" min="1" value={model.requests_per_minute} onChange={(event) => update(index, { requests_per_minute: Number(event.target.value) })} required /></label>
        <label>令牌/分钟<input type="number" min="1" value={model.tokens_per_minute} onChange={(event) => update(index, { tokens_per_minute: Number(event.target.value) })} required /></label>
        <label>超时毫秒<input type="number" min="1" value={model.timeout_ms} onChange={(event) => update(index, { timeout_ms: Number(event.target.value) })} required /></label>
        <label>Fallback 组<input value={model.fallback_group ?? ''} onChange={(event) => update(index, { fallback_group: event.target.value || null })} /></label>
      </div>
      <div className="button-row model-actions"><label className="checkbox-label"><input type="checkbox" checked={model.tool_use} onChange={(event) => update(index, { tool_use: event.target.checked })} />允许工具调用</label><label className="checkbox-label"><input type="radio" name="default-model" checked={model.default} onChange={() => makeDefault(index)} />设为默认</label>{models.length > 1 && <button type="button" className="secondary-button" onClick={() => setModels((current) => current.filter((_, position) => position !== index))}>移除模型</button>}</div>
    </fieldset>)}
    <div className="button-row"><button type="button" className="secondary-button" disabled={busy || uncertain} onClick={() => setModels((current) => [...current, { ...emptyModel(), id: `model:${current.length + 1}`, default: false }])}>添加模型</button><button type="submit" className="primary-button" disabled={busy || !config}>{busy ? '正在保存…' : uncertain ? '核实原保存' : '保存模型配置'}</button></div>
  </form>;
}
