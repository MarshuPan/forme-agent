import { useEffect, useRef, useState } from 'react';
import { ApiError, newKey } from '../api/client.ts';
import type { WorkbenchClient } from '../api/client.ts';
import { channelSettingsPatch, channelSettingsDigest, verifyChannelOperation } from '../channel-settings.ts';
import type { ChannelConfig, ChannelSettingsPatch, ChannelSettingsView } from '../channel-settings.ts';

export function ChannelSettings({ api, revision, disabled, onSaved }: {
  api: WorkbenchClient; revision: number | undefined; disabled: boolean; onSaved: () => void;
}) {
  const [view,setView] = useState<ChannelSettingsView | null>(null), [channels,setChannels] = useState<ChannelConfig[]>([]);
  const [password,setPassword] = useState(''), [issue,setIssue] = useState('');
  const [loading,setLoading] = useState(true), [busy,setBusy] = useState(false), [unknown,setUnknown] = useState(false);
  const running = useRef(false), generation = useRef(0);
  const pending = useRef<{patch:ChannelSettingsPatch;digest:string} | null>(null);
  async function load() {
    const current = ++generation.current; setLoading(true); setIssue('');
    try {
      const next = await api.channelSettings();
      if (current !== generation.current) return;
      setView(next); if (!pending.current) setChannels(structuredClone(next.channels));
    } catch (error) { if (current === generation.current) {setView(null);setIssue(error instanceof Error ? error.message : '渠道设置不可用。');} }
    finally {if (current === generation.current) setLoading(false);}
  }
  useEffect(() => {void load();return () => {generation.current++;};},[api,revision]);
  async function save() {
    if (!view || disabled || loading || running.current) return;
    running.current = true; setBusy(true); setIssue('');
    const current = generation.current, recovering = Boolean(pending.current);
    try {
      let next: ChannelSettingsView;
      if (pending.current) {
        const original = pending.current;
        verifyChannelOperation(await api.channelOperation(original.patch.operation_id),original.patch,original.digest);
        next = await api.channelSettings();
      } else {
        const patch = channelSettingsPatch(view,channels,newKey('channel-settings'));
        pending.current = {patch,digest:await channelSettingsDigest(patch)};
        next = await api.saveChannelSettings(patch,password);
      }
      if (current !== generation.current) return;
      pending.current = null; setUnknown(false);setView(next);setChannels(structuredClone(next.channels));setIssue('渠道设置已保存。');onSaved();
    } catch (error) {
      if (current !== generation.current) return;
      if (!recovering && error instanceof ApiError && [400,401,403,409,422,426].includes(error.status)) pending.current = null;
      setUnknown(Boolean(pending.current));setIssue(error instanceof Error ? error.message : '保存结果尚未确认。');
    } finally {running.current = false;setBusy(false);setPassword('');}
  }
  const locked = disabled || loading || busy || unknown;
  const unused = view?.sources.filter((s) => !channels.some((p) => p.channel_ref === s.channel_ref)) ?? [];
  const update = (index: number, value: Partial<ChannelConfig>) => setChannels((all) => all.map((p,i) => i === index ? {...p,...value} : p));
  return <section className="model-profile"><h3>渠道</h3><p className="muted">接收已连接渠道的文本消息，由主脑持续处理。</p>
    <button className="secondary-button" disabled={disabled || busy || loading} onClick={() => void load()}>重新读取渠道状态</button>
    {loading && <p role="status">正在读取…</p>}
    {view && <><p className="notice">{!view.configured ? '请先保存模型配置。' : view.pending_restart ? '设置等待主脑应用。' : '渠道设置已应用。'}</p>
      {!view.sources_available && <p role="status" className="notice">本机渠道来源暂时不可用。仍可禁用或移除已有配置。</p>}
      {view.sources_available && view.sources.length === 0 && <p className="notice">当前安装尚未提供渠道来源。</p>}
      <form onSubmit={(e) => {e.preventDefault();void save();}} aria-busy={busy}>
        {channels.map((p,index) => <fieldset key={p.channel_ref} className="model-profile" disabled={locked || !view.configured}>
          <legend>{p.channel_ref}</legend><p className="muted">来源：{p.connector_ref} · 范围：{p.scope}</p>
          <label className="checkbox-label"><input type="checkbox" checked={p.enabled} disabled={!view.sources_available && !p.enabled} onChange={(e) => update(index,{enabled:e.target.checked})} />启用</label>
          <button type="button" className="secondary-button" onClick={() => setChannels((all) => all.filter((_,i) => i !== index))}>移除配置</button>
        </fieldset>)}
        <label>添加渠道<select value="" disabled={locked || !view.configured || unused.length === 0} onChange={(e) => {
          const source = unused.find((s) => s.channel_ref === e.target.value);if (source) setChannels((all) => [...all,structuredClone(source)]);
        }}><option value="">选择本机渠道</option>{unused.map((s) => <option key={s.channel_ref} value={s.channel_ref}>{s.channel_ref} · {s.scope}</option>)}</select></label>
        {!unknown && <label>确认登录密码<input type="password" autoComplete="current-password" maxLength={128} required disabled={locked} value={password} onChange={(e) => setPassword(e.target.value)} /></label>}
        <button className="secondary-button" disabled={disabled || loading || busy || !view.configured}>{unknown ? '核实原保存操作' : '保存渠道设置'}</button>
      </form></>}
    {issue && <p role="status" className="notice">{issue}</p>}
  </section>;
}
