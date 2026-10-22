import { useEffect, useRef, useState } from 'react';
import type { FormEvent } from 'react';
import { ApiError, newKey } from '../api/client.ts';
import type { WorkbenchClient } from '../api/client.ts';
import type { ProductConfigView, ProductModelProfilePatch, RuntimeView } from '../types.ts';
import { draftFromConfig, emptyModel, patchFromDraft } from '../configuration.ts';
import type { ModelDraft } from '../configuration.ts';
import { ConfigurationActivation } from './ConfigurationActivation.tsx';
import { DataManagement } from './DataManagement.tsx';


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
    </form></article></div><DataManagement api={api} connected={Boolean(runtime)} />
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
