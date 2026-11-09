import { useEffect, useRef, useState } from 'react';
import type { FormEvent } from 'react';
import { ApiError, newKey } from '../api/client.ts';
import type { WorkbenchClient } from '../api/client.ts';
import { computerSettingsDigest, computerSettingsPatch, verifyComputerSettingsOperation } from '../computer-settings.ts';
import type { ComputerSettingsPatch, ComputerSettingsView } from '../computer-settings.ts';

export function ComputerSettings({ api, revision, disabled, onSaved }: {
  api: WorkbenchClient; revision: number | undefined; disabled: boolean; onSaved: () => void;
}) {
  const [view, setView] = useState<ComputerSettingsView | null>(null);
  const [enabled, setEnabled] = useState(false), [password, setPassword] = useState('');
  const [busy, setBusy] = useState(false), [loading, setLoading] = useState(true);
  const [issue, setIssue] = useState(''), [unknown, setUnknown] = useState(false);
  const pending = useRef<{ patch: ComputerSettingsPatch; digest: string } | null>(null);
  const live = useRef(true);
  useEffect(() => { live.current = true; return () => { live.current = false; }; }, []);
  useEffect(() => {
    let active = true;
    setLoading(true);
    api.computerSettings().then((next) => {
      if (!active) return;
      setView(next); if (!pending.current) setEnabled(next.computer.enabled);
    }).catch((error: unknown) => { if (active) setIssue(error instanceof Error ? error.message : '桌面设置不可用。'); })
      .finally(() => { if (active) setLoading(false); });
    return () => { active = false; };
  }, [api, revision]);
  async function recover() {
    const original = pending.current;
    if (!original) return;
    verifyComputerSettingsOperation(await api.computerSettingsOperation(original.patch.operation_id), original.patch, original.digest);
    const next = await api.computerSettings();
    if (!live.current) return;
    pending.current = null; setUnknown(false); setView(next); setEnabled(next.computer.enabled);
    setIssue('已确认原保存操作。'); onSaved();
  }
  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault(); if (!view || busy || disabled || loading) return;
    const resolving = Boolean(pending.current);
    setBusy(true); setIssue('');
    try {
      if (pending.current) { await recover(); return; }
      const patch = computerSettingsPatch(view,enabled,newKey('computer-settings'));
      const digest = await computerSettingsDigest(patch);
      pending.current = { patch, digest };
      const next = await api.saveComputerSettings(patch,password);
      if (!live.current) return;
      pending.current = null; setUnknown(false); setView(next); setEnabled(next.computer.enabled);
      setIssue('桌面设置已保存。'); onSaved();
    } catch (error) {
      if (!live.current) return;
      if (!resolving && error instanceof ApiError && [400,401,403,409,422,426].includes(error.status)) {
        pending.current = null; setUnknown(false);
      } else if (pending.current) {
        setUnknown(true);
        try { await recover(); return; } catch { /* Original request remains unresolved. */ }
      }
      setIssue(error instanceof Error ? error.message : '保存结果尚未确认，请核实原操作。');
    } finally { if (live.current) { setBusy(false); setPassword(''); } }
  }
  return <section className="model-profile"><h3>桌面操作</h3>
    <p className="muted">允许 forme 在你选择并授权的桌面会话中处理任务。具体操作仍需确认。</p>
    {loading && <p role="status">正在读取桌面设置…</p>}
    {view && <>
      <p className="notice">{!view.available ? '当前安装尚未提供桌面操作。'
        : !view.configured ? '请先保存模型配置。'
        : view.pending_restart ? '配置已保存，等待主脑应用。'
        : view.computer.enabled ? '桌面操作已启用。' : '桌面操作已关闭。'}</p>
      <form onSubmit={submit} aria-busy={busy}>
        <label className="checkbox-label"><input type="checkbox" checked={enabled}
          disabled={disabled || busy || loading || unknown || !view.configured || (!view.available && !enabled)}
          onChange={(event) => setEnabled(event.target.checked)} />启用桌面操作</label>
        {!unknown && <label>确认登录密码<input type="password" autoComplete="current-password" value={password}
          onChange={(event) => setPassword(event.target.value)} required maxLength={128} disabled={disabled || busy || loading} /></label>}
        <button className="secondary-button" disabled={disabled || busy || loading || !view.configured || (!unknown && enabled === view.computer.enabled)}>
          {busy ? '正在核实…' : unknown ? '核实原保存操作' : '保存桌面设置'}</button>
      </form>
    </>}
    {issue && <p role="status" className="notice">{issue}</p>}
    {unknown && <p role="status">保存结果尚未确认。此处只查询原操作，不会再次提交。</p>}
  </section>;
}
