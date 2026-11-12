import { useEffect, useRef, useState } from 'react';
import { ApiError, newKey } from '../api/client.ts';
import type { WorkbenchClient } from '../api/client.ts';
import { pluginSettingsPatch, pluginSettingsDigest, verifyPluginOperation } from '../plugin-settings.ts';
import type { PluginConfig, PluginSettingsPatch, PluginSettingsView } from '../plugin-settings.ts';

export function PluginSettings({ api, revision, disabled, onSaved }: {
  api: WorkbenchClient; revision: number | undefined; disabled: boolean; onSaved: () => void;
}) {
  const [view,setView] = useState<PluginSettingsView | null>(null), [plugins,setPlugins] = useState<PluginConfig[]>([]);
  const [password,setPassword] = useState(''), [issue,setIssue] = useState('');
  const [loading,setLoading] = useState(true), [busy,setBusy] = useState(false), [unknown,setUnknown] = useState(false);
  const running = useRef(false), generation = useRef(0);
  const pending = useRef<{patch:PluginSettingsPatch;digest:string} | null>(null);
  async function load() {
    const current = ++generation.current; setLoading(true); setIssue('');
    try {
      const next = await api.pluginSettings();
      if (current !== generation.current) return;
      setView(next); if (!pending.current) setPlugins(structuredClone(next.plugins));
    } catch (error) { if (current === generation.current) {setView(null);setIssue(error instanceof Error ? error.message : '插件设置不可用。');} }
    finally {if (current === generation.current) setLoading(false);}
  }
  useEffect(() => {void load();return () => {generation.current++;};},[api,revision]);
  async function save() {
    if (!view || disabled || loading || running.current) return;
    running.current = true; setBusy(true); setIssue('');
    const current = generation.current, recovering = Boolean(pending.current);
    try {
      let next: PluginSettingsView;
      if (pending.current) {
        const original = pending.current;
        verifyPluginOperation(await api.pluginOperation(original.patch.operation_id),original.patch,original.digest);
        next = await api.pluginSettings();
      } else {
        const patch = pluginSettingsPatch(view,plugins,newKey('plugin-settings'));
        pending.current = {patch,digest:await pluginSettingsDigest(patch)};
        next = await api.savePluginSettings(patch,password);
      }
      if (current !== generation.current) return;
      pending.current = null; setUnknown(false);setView(next);setPlugins(structuredClone(next.plugins));setIssue('插件设置已保存。');onSaved();
    } catch (error) {
      if (current !== generation.current) return;
      if (!recovering && error instanceof ApiError && [400,401,403,409,422,426].includes(error.status)) pending.current = null;
      setUnknown(Boolean(pending.current));setIssue(error instanceof Error ? error.message : '保存结果尚未确认。');
    } finally {running.current = false;setBusy(false);setPassword('');}
  }
  const locked = disabled || loading || busy || unknown;
  const unused = view?.sources.filter((s) => !plugins.some((p) => p.plugin_ref === s.plugin_ref)) ?? [];
  const update = (index: number, value: Partial<PluginConfig>) => setPlugins((all) => all.map((p,i) => i === index ? {...p,...value} : p));
  return <section className="model-profile"><h3>插件</h3><p className="muted">添加本机提供的技能与工具插件，让任务按需使用相应方法、知识和已授权工具。</p>
    <button className="secondary-button" disabled={disabled || busy || loading} onClick={() => void load()}>重新读取插件状态</button>
    {loading && <p role="status">正在读取…</p>}
    {view && <><p className="notice">{!view.configured ? '请先保存模型配置。' : view.pending_restart ? '设置等待主脑应用。' : '插件设置已应用。'}</p>
      {!view.sources_available && <p role="status" className="notice">本机插件来源暂时不可用。仍可禁用或移除已有配置。</p>}
      {view.sources_available && view.sources.length === 0 && <p className="notice">当前安装尚未提供插件来源。</p>}
      <form onSubmit={(e) => {e.preventDefault();void save();}} aria-busy={busy}>
        {plugins.map((p,index) => <fieldset key={p.plugin_ref} className="model-profile" disabled={locked || !view.configured}>
          <legend>{p.plugin_ref}</legend><p className="muted">来源：{p.source_ref} · 范围：{p.scope}</p>
          <label className="checkbox-label"><input type="checkbox" checked={p.trust === 'owner_input'} onChange={(e) => update(index,{trust:e.target.checked ? 'owner_input' : 'untrusted'})} />我信任此来源</label>
          <label className="checkbox-label"><input type="checkbox" checked={p.enabled} disabled={!view.sources_available && !p.enabled} onChange={(e) => update(index,{enabled:e.target.checked})} />启用</label>
          <button type="button" className="secondary-button" onClick={() => setPlugins((all) => all.filter((_,i) => i !== index))}>移除配置</button>
        </fieldset>)}
        <label>添加插件<select value="" disabled={locked || !view.configured || unused.length === 0} onChange={(e) => {
          const source = unused.find((s) => s.source_ref === e.target.value);if (source) setPlugins((all) => [...all,structuredClone(source)]);
        }}><option value="">选择本机插件</option>{unused.map((s) => <option key={s.source_ref} value={s.source_ref}>{s.plugin_ref} · {s.scope}</option>)}</select></label>
        {!unknown && <label>确认登录密码<input type="password" autoComplete="current-password" maxLength={128} required disabled={locked} value={password} onChange={(e) => setPassword(e.target.value)} /></label>}
        <button className="secondary-button" disabled={disabled || loading || busy || !view.configured}>{unknown ? '核实原保存操作' : '保存插件设置'}</button>
      </form></>}
    {issue && <p role="status" className="notice">{issue}</p>}
  </section>;
}
