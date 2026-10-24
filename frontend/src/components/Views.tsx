import { useEffect, useRef, useState } from 'react';
import type { FormEvent } from 'react';
import { ApiError, newKey } from '../api/client.ts';
import type { WorkbenchClient } from '../api/client.ts';
import type { ProductBrainPreferences, ProductCapabilitySettings, ProductConfigView, ProductModelProfilePatch, ProductNetworkConfig, RuntimeView } from '../types.ts';
import { draftFromConfig, emptyModel, patchFromDraft } from '../configuration.ts';
import type { ModelDraft } from '../configuration.ts';
import { capabilitiesFromDraft, draftFromCapabilities } from '../skills.ts';
import type { SkillDraft } from '../skills.ts';
import { ConfigurationActivation } from './ConfigurationActivation.tsx';
import { NetworkConfigurationActivation } from './NetworkConfigurationActivation.tsx';
import { DataManagement } from './DataManagement.tsx';
import { Maintenance } from './Maintenance.tsx';
import { RuntimeDiagnostics } from './RuntimeDiagnostics.tsx';
import { BrainPreferences } from './BrainPreferences.tsx';
import { SkillSettings } from './SkillSettings.tsx';
import type { MaterialLifecycleTarget } from '../memory-material.ts';


export function Settings({ api, runtime, issue, refresh, signedOut, materialTarget, consumeMaterialTarget }: { api: WorkbenchClient; runtime: RuntimeView | null; issue: string; refresh: () => void; signedOut: () => void; materialTarget: MaterialLifecycleTarget | null; consumeMaterialTarget: () => void }) {
  const [busy, setBusy] = useState(false), [error, setError] = useState('');
  const [passwordUnknown, setPasswordUnknown] = useState(false);
  const passwordOperation = useRef<string | null>(null);
  const [config, setConfig] = useState<ProductConfigView | null>(null), [configError, setConfigError] = useState('');
  const [configLoading, setConfigLoading] = useState(true), [configBusy, setConfigBusy] = useState(false);
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
      const operation = passwordOperation.current ?? newKey('password');
      passwordOperation.current = operation;
      await api.password(String(data.get('current')), String(data.get('next')), operation);
      passwordOperation.current = null; setPasswordUnknown(false); form.reset(); signedOut();
    } catch (problem) {
      if (problem instanceof ApiError && problem.status === 0) setPasswordUnknown(true);
      else { passwordOperation.current = null; setPasswordUnknown(false); }
      setError(problem instanceof Error ? problem.message : '密码修改尚未确认。');
    }
    finally { setBusy(false); }
  }
  return <section className="view"><header className="view-header"><div><p className="eyebrow">这台 forme 的运行情况</p><h1>设置与维护</h1></div><button className="secondary-button" onClick={refresh}>重新检查连接</button></header>
    <div className="settings-grid"><article className="surface"><h2>主脑服务</h2><p className="status-pill">{runtime ? '已连接正式运行时' : '主脑不可用'}</p><p className="muted">{issue || '页面与已授权后台任务分别运行。'}</p>
      {runtime && <><dl><div><dt>工作空间</dt><dd>{runtime.workspace}</dd></div><div><dt>可用模型配置</dt><dd>{runtime.model_profiles.join('、')}</dd></div></dl><ul className="service-list">{runtime.service_states.map((service) => <li key={service.name}><span>{service.name}</span><span>{service.status}</span></li>)}</ul></>}
      {!runtime && <p className="notice">账号服务仍可使用。请检查本机主脑服务和安装配置；这里不会另建一个空主脑。</p>}
    </article><RuntimeDiagnostics api={api} runtime={runtime} /><article className="surface"><h2>主脑与入口配置</h2><p className="muted">密钥只写入本机受保护存储，不会回显到页面、浏览器存储或日志。</p>
      <ProductConfigEditor api={api} config={config} loading={configLoading} error={configError} busy={configBusy} onReload={() => void loadConfig()} onBusy={setConfigBusy} onError={setConfigError} onSaved={setConfig} />
      <ConfigurationActivation api={api} config={config} coreConnected={Boolean(runtime)} onSaved={setConfig} refresh={refresh} />
      <NetworkConfigurationActivation api={api} config={config} onSaved={setConfig} refresh={refresh} />
    </article><article className="surface"><h2>修改登录密码</h2><p className="muted">成功后撤销已有登录会话，脑数据与所有者身份保持不变。</p><form onSubmit={changePassword} aria-busy={busy}>
      <label>当前密码<input name="current" type="password" autoComplete="current-password" required maxLength={128} readOnly={busy || passwordUnknown} /></label>
      <label>新密码<input name="next" type="password" autoComplete="new-password" required minLength={12} maxLength={128} readOnly={busy || passwordUnknown} /></label>
      <label>确认新密码<input name="confirm" type="password" autoComplete="new-password" required minLength={12} maxLength={128} readOnly={busy || passwordUnknown} /></label>
      {error && <p role="alert" className="form-error">{error}</p>}<button className="primary-button" disabled={busy}>{busy ? '正在确认…' : passwordUnknown ? '使用原操作继续核实' : '更新密码并重新登录'}</button>
    </form></article><Maintenance api={api} refresh={refresh} /></div><DataManagement api={api} connected={Boolean(runtime)} materialTarget={materialTarget} consumeMaterialTarget={consumeMaterialTarget} />
  </section>;
}

function ProductConfigEditor({ api, config, loading, error, busy, onReload, onBusy, onError, onSaved }: {
  api: WorkbenchClient; config: ProductConfigView | null; loading: boolean; error: string; busy: boolean;
  onReload: () => void; onBusy: (value: boolean) => void; onError: (value: string) => void;
  onSaved: (value: ProductConfigView) => void;
}) {
  const [models, setModels] = useState<ModelDraft[]>(() => draftFromConfig(config));
  const [brain, setBrain] = useState<ProductBrainPreferences | null>(config?.brain ?? null);
  const [skills, setSkills] = useState<SkillDraft[]>(() => draftFromCapabilities(config?.capabilities));
  const [network, setNetwork] = useState<ProductNetworkConfig>(() => config?.network ?? loopbackNetwork());
  const [loadedRevision, setLoadedRevision] = useState<number | null>(null);
  const pending = useRef<{ operation: string; revision: number; models: ProductModelProfilePatch[]; brain: ProductBrainPreferences; capabilities: ProductCapabilitySettings; network: ProductNetworkConfig } | null>(null);
  const [uncertain, setUncertain] = useState(false);
  useEffect(() => {
    if (config && config.active_revision !== loadedRevision) {
      setModels(draftFromConfig(config)); setBrain(config.brain); setSkills(draftFromCapabilities(config.capabilities)); setNetwork(config.network); setLoadedRevision(config.active_revision);
    }
  }, [config, loadedRevision]);
  function update(index: number, change: Partial<ModelDraft>) {
    setModels((current) => current.map((model, position) => position === index ? { ...model, ...change } : model));
  }
  function makeDefault(index: number) {
    setModels((current) => current.map((model, position) => ({ ...model, default: position === index })));
  }
  async function save(event: FormEvent<HTMLFormElement>) {
    event.preventDefault(); if (busy || !config || !models.length || !brain) return;
    onBusy(true); onError('');
    try {
      pending.current ??= { operation: newKey('config'), revision: config.active_revision, models: patchFromDraft(models), brain, capabilities: capabilitiesFromDraft(skills), network };
      const { operation, revision, models: patch, brain: preferences, capabilities, network: networkPatch } = pending.current;
      const result = await api.saveProductConfig(revision, patch, operation, preferences, capabilities, networkPatch);
      pending.current = null; setUncertain(false);
      onSaved(result.view); setLoadedRevision(result.view.active_revision); setModels(draftFromConfig(result.view)); setBrain(result.view.brain); setSkills(draftFromCapabilities(result.view.capabilities)); setNetwork(result.view.network);
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
    {config?.pending_restart && <p className="notice">配置版本 {config.active_revision} 已保存。主脑确认版本：{config.acknowledged_revision ?? '无'}；Web 网络投影{config.network_active ? '已发布' : '待单独应用'}。两项分别核实。</p>}
    {uncertain && <p className="notice">上次保存结果尚未确认，再次提交将核实同一份配置；请先完成核实再修改。</p>}
    {error && <p className="form-error" role="alert">{error}</p>}
    {brain && <BrainPreferences value={brain} models={models} disabled={busy || uncertain} onChange={setBrain} />}
    <SkillSettings value={skills} disabled={busy || uncertain} onChange={setSkills} />
    <NetworkSettings value={network} active={config?.network_active ?? true} disabled={busy || uncertain} onChange={setNetwork} />
    {config && <p className="muted">工作空间与分工来源：{config.brain_source === 'installation_template' ? '安装默认值' : '已保存配置'}。</p>}
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
    <div className="button-row"><button type="button" className="secondary-button" disabled={busy || uncertain} onClick={() => setModels((current) => [...current, { ...emptyModel(), id: `model:${current.length + 1}`, default: false }])}>添加模型</button><button type="submit" className="primary-button" disabled={busy || !config}>{busy ? '正在保存…' : uncertain ? '核实原保存' : '保存模型与主脑配置'}</button></div>
  </form>;
}

function loopbackNetwork(): ProductNetworkConfig {
  return { schema_version: 1, bind: '127.0.0.1:8765', allowed_hosts: [], tls: null, firewall: null };
}

function NetworkSettings({ value, active, disabled, onChange }: { value: ProductNetworkConfig; active: boolean; disabled: boolean; onChange: (value: ProductNetworkConfig) => void }) {
  const lan = !isLoopback(value.bind);
  const hosts = value.allowed_hosts.join('\n');
  function update(change: Partial<ProductNetworkConfig>) { onChange({ ...value, ...change }); }
  function updateTls(change: Partial<NonNullable<ProductNetworkConfig['tls']>>) {
    onChange({ ...value, tls: { schema_version: 1, certificate_path: '', private_key_path: '', ...value.tls, ...change } });
  }
  return <section className="network-settings"><h3>Web 网络入口</h3>
    <p className="muted">默认只监听本机。启用 LAN 时必须使用明确的 IP、HTTPS、精确 Host（含端口）和已验证防火墙规则；保存后还要单独应用。规则由管理员预先配置，ForMe 不自动修改防火墙。</p>
    <p className={active ? 'status-pill' : 'notice'}>{active ? '当前网络配置已发布，监听与连通状态需另行核实' : '网络配置已保存，尚未发布'}</p>
    <div className="form-grid">
      <label>监听地址<input value={value.bind} onChange={(event) => update({ bind: event.target.value, ...(isLoopback(event.target.value) ? { firewall: null } : {}) })} required disabled={disabled} placeholder="127.0.0.1:8765" /></label>
      <label className="wide-field">允许的 Host（每行一个，必须含端口）<textarea value={hosts} onChange={(event) => update({ allowed_hosts: event.target.value.split(/[,\n]/).map((host) => host.trim()).filter(Boolean) })} disabled={disabled} placeholder={lan ? '192.168.1.20:8765' : '留空表示本机 HTTP'} rows={2} /></label>
    </div>
    <label className="checkbox-label"><input type="checkbox" checked={value.tls !== null} onChange={(event) => event.target.checked ? updateTls({}) : update({ tls: null, allowed_hosts: [] })} disabled={disabled} />启用 HTTPS（LAN 必须启用）</label>
    {value.tls && <div className="form-grid">
      <label className="wide-field">证书路径<input value={value.tls.certificate_path} onChange={(event) => updateTls({ certificate_path: event.target.value })} required disabled={disabled} /></label>
      <label className="wide-field">私钥路径<input value={value.tls.private_key_path} onChange={(event) => updateTls({ private_key_path: event.target.value })} required disabled={disabled} /></label>
    </div>}
    {lan && <label>防火墙规则标识<input value={value.firewall?.rule_ref ?? ''} onChange={(event) => update({ firewall: event.target.value ? { schema_version: 1, rule_ref: event.target.value } : null })} required disabled={disabled} placeholder="firewall:forme-web-lan" /></label>}
    {!lan && <p className="muted">当前是 loopback；不会要求防火墙规则。切换到 LAN 前请先准备受管 Web 证书材料。</p>}
  </section>;
}

function isLoopback(bind: string): boolean {
  if (/^\[::1\]:[1-9]\d{0,4}$/.test(bind)) return true;
  const match = /^127\.(\d{1,3})\.(\d{1,3})\.(\d{1,3}):([1-9]\d{0,4})$/.exec(bind);
  return Boolean(match && match.slice(1, 4).every((octet) => Number(octet) <= 255) && Number(match[4]) <= 65535);
}
