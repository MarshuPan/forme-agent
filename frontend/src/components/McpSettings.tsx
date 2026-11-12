import { useEffect, useRef, useState } from 'react';
import { ApiError, newKey } from '../api/client.ts';
import type { WorkbenchClient } from '../api/client.ts';
import { mcpSettingsDigest, mcpSettingsPatch, verifyMcpSettingsOperation } from '../mcp-settings.ts';
import type { McpProgramCatalog, McpServer, McpSettingsPatch, McpSettingsView } from '../mcp-settings.ts';
import { McpPrograms } from './McpPrograms.tsx';
import { McpDiscovery } from './McpDiscovery.tsx';

export function McpSettings({ api, revision, disabled, connected, onSaved }: {
  api: WorkbenchClient; revision: number | undefined; disabled: boolean; connected: boolean; onSaved: () => void;
}) {
  const [view,setView] = useState<McpSettingsView | null>(null), [catalog,setCatalog] = useState<McpProgramCatalog | null>(null);
  const [entries,setEntries] = useState<McpServer[]>([]), [password,setPassword] = useState('');
  const [loading,setLoading] = useState(true), [busy,setBusy] = useState(false), [unknown,setUnknown] = useState(false);
  const [issue,setIssue] = useState(''), [catalogIssue,setCatalogIssue] = useState('');
  const pending = useRef<{patch:McpSettingsPatch;digest:string} | null>(null), running = useRef(false);
  const generation = useRef(0);
  async function load() {
    const current = ++generation.current;
    setLoading(true); setIssue(''); setCatalogIssue('');
    const [settings, programs] = await Promise.allSettled([api.mcpSettings(),api.mcpPrograms()]);
    if (current !== generation.current) return;
    if (settings.status === 'fulfilled') { setView(settings.value); if (!pending.current) setEntries(structuredClone(settings.value.mcp_servers)); }
    else { setView(null); setIssue(settings.reason instanceof Error ? settings.reason.message : 'MCP 设置不可用。'); }
    if (programs.status === 'fulfilled') { setCatalog(programs.value); }
    else { setCatalog(null); setCatalogIssue(programs.reason instanceof Error ? programs.reason.message : '程序目录不可用。'); }
    setLoading(false);
  }
  useEffect(() => { void load(); return () => { generation.current++; }; }, [api,revision]);
  async function save() {
    if (!view || disabled || loading || running.current) return;
    running.current = true; setBusy(true); setIssue('');
    const recovering = Boolean(pending.current), current = generation.current;
    try {
      let next: McpSettingsView;
      if (pending.current) {
        const original = pending.current;
        verifyMcpSettingsOperation(await api.mcpSettingsOperation(original.patch.operation_id),original.patch,original.digest);
        next = await api.mcpSettings();
      } else {
        const cleaned = entries.map((s) => ({...s,allowlist:{...s.allowlist,
          tools:s.allowlist.tools.map((v) => v.trim()).filter(Boolean),resources:s.allowlist.resources.map((v) => v.trim()).filter(Boolean)}}));
        const patch = mcpSettingsPatch(view,cleaned,newKey('mcp-settings'));
        pending.current = {patch,digest:await mcpSettingsDigest(patch)};
        next = await api.saveMcpSettings(patch,password);
      }
      if (current !== generation.current) return;
      pending.current = null; setUnknown(false); setView(next); setEntries(structuredClone(next.mcp_servers));
      setIssue('MCP 设置已保存。'); onSaved();
    } catch (error) {
      if (current !== generation.current) return;
      if (!recovering && error instanceof ApiError && [400,401,403,409,422,426].includes(error.status)) pending.current = null;
      setUnknown(Boolean(pending.current)); setIssue(error instanceof Error ? error.message : '保存结果尚未确认。');
    } finally { running.current = false; setBusy(false); setPassword(''); }
  }
  const locked = disabled || loading || busy || unknown;
  function update(index: number, value: Partial<McpServer>) { setEntries((all) => all.map((s,i) => i === index ? {...s,...value} : s)); }
  function add() {
    const first = catalog?.programs.find((p) => !p.revoked);
    if (!first) return;
    setEntries((all) => [...all,{schema_version:1,server_ref:newKey('mcp'),program_ref:first.program_ref,scope:first.scope,
      allowlist:{schema_version:1,tools:[],resources:[]},enabled:false,trust:'untrusted',timeout:10_000,output_budget:131_072}]);
  }
  return <section className="model-profile"><h3>MCP 工具</h3><p className="muted">连接本机 MCP 程序，为任务提供允许使用的工具。保存后可发起工具发现并确认具体操作。</p>
    <button className="secondary-button" disabled={disabled || busy || loading} onClick={() => void load()}>重新读取 MCP 状态</button>
    {loading && <p role="status">正在读取…</p>}{catalogIssue && <p role="status" className="notice">{catalogIssue}</p>}
    {catalog && <McpPrograms api={api} catalog={catalog} disabled={locked} changed={load} />}
    {view && <><p className="notice">{!view.configured ? '请先保存模型配置。' : view.pending_restart ? '设置等待主脑应用。'
      : view.available ? '当前安装支持受管 MCP。' : '当前安装的 MCP 执行服务不可用。'}</p>
      <form onSubmit={(e) => { e.preventDefault(); void save(); }} aria-busy={busy}>
        {entries.map((s,index) => <fieldset key={index} disabled={locked || !view.configured} className="model-profile">
          <legend>MCP {index + 1}</legend>
          <label>名称<input value={s.server_ref} maxLength={128} onChange={(e) => update(index,{server_ref:e.target.value})} required /></label>
          <label>已登记程序<select value={s.program_ref} onChange={(e) => { const p = catalog?.programs.find((p) => p.program_ref === e.target.value);
            if (p) update(index,{program_ref:p.program_ref,scope:p.scope}); }}>
            {!catalog?.programs.some((p) => !p.revoked && p.program_ref === s.program_ref) && <option value={s.program_ref}>原程序当前不可用</option>}
            {catalog?.programs.filter((p) => !p.revoked).map((p) => <option key={p.program_ref} value={p.program_ref}>{p.source_ref} · {p.scope}</option>)}
          </select></label><p className="muted">工作范围：{s.scope}</p>
          <label>允许的工具名称（每行一个）<textarea value={s.allowlist.tools.join('\n')} onChange={(e) => update(index,{allowlist:{...s.allowlist,tools:e.target.value.split('\n')}})} /></label>
          <label>允许的资源（每行一个）<textarea value={s.allowlist.resources.join('\n')} onChange={(e) => update(index,{allowlist:{...s.allowlist,resources:e.target.value.split('\n')}})} /></label>
          <label>超时（毫秒）<input type="number" min={1} max={60000} value={s.timeout} onChange={(e) => update(index,{timeout:Number(e.target.value)})} /></label>
          <label>最大结果大小（字节）<input type="number" min={1} max={1048576} value={s.output_budget} onChange={(e) => update(index,{output_budget:Number(e.target.value)})} /></label>
          <label className="checkbox-label"><input type="checkbox" checked={s.trust === 'owner_input'} onChange={(e) => update(index,{trust:e.target.checked ? 'owner_input' : 'untrusted'})} />我信任此来源</label>
          <label className="checkbox-label"><input type="checkbox" checked={s.enabled} disabled={!view.available && !s.enabled} onChange={(e) => update(index,{enabled:e.target.checked})} />启用</label>
          <button type="button" className="secondary-button" onClick={() => setEntries((all) => all.filter((_,i) => i !== index))}>移除配置</button>
        </fieldset>)}
        <button type="button" className="secondary-button" disabled={locked || !view.configured || entries.length >= 64 || !catalog?.programs.some((p) => !p.revoked)} onClick={add}>添加 MCP</button>
        {!unknown && <label>确认登录密码<input type="password" autoComplete="current-password" maxLength={128} value={password} disabled={locked} onChange={(e) => setPassword(e.target.value)} required /></label>}
        <button className="secondary-button" disabled={disabled || busy || loading || !view.configured}>{unknown ? '核实原保存操作' : '保存 MCP 设置'}</button>
      </form>
      {view.owner && catalog && view.mcp_servers.map((server) => <McpDiscovery key={server.server_ref} api={api} owner={view.owner!} server={server}
        program={catalog.programs.find((p) => p.program_ref === server.program_ref)} disabled={locked || !connected || view.pending_restart} />)}
    </>}{issue && <p role="status" className="notice">{issue}</p>}
  </section>;
}
