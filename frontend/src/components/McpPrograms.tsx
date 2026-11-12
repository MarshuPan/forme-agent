import { useRef, useState } from 'react';
import { ApiError, newKey } from '../api/client.ts';
import type { WorkbenchClient } from '../api/client.ts';
import { mcpProgramCommand, mcpProgramDigest, verifyMcpProgramOperation } from '../mcp-settings.ts';
import type { McpProgramCatalog, McpProgramCommand } from '../mcp-settings.ts';

export function McpPrograms({ api, catalog, disabled, changed }: {
  api: WorkbenchClient; catalog: McpProgramCatalog; disabled: boolean; changed: () => Promise<void>;
}) {
  const [choice,setChoice] = useState(''), [password,setPassword] = useState('');
  const [busy,setBusy] = useState(false), [issue,setIssue] = useState(''), [unknown,setUnknown] = useState(false);
  const running = useRef(false);
  const pending = useRef<{command:McpProgramCommand;digest:string} | null>(null);
  async function perform(action?: McpProgramCommand['action']) {
    if (disabled || running.current) return;
    running.current = true; setBusy(true); setIssue('');
    const recovering = Boolean(pending.current);
    try {
      if (!pending.current) {
        if (!action) return;
        const command = mcpProgramCommand(catalog,action,newKey('mcp-program'),Date.now());
        pending.current = {command,digest:await mcpProgramDigest(command)};
      }
      const original = pending.current;
      const receipt = recovering ? await api.mcpProgramOperation(original.command.operation_id)
        : await api.registerMcpProgram(original.command,password);
      verifyMcpProgramOperation(receipt,original.command,original.digest);
      pending.current = null; setUnknown(false); setIssue('程序登记已确认。');
      await changed();
    } catch (error) {
      if (!recovering && error instanceof ApiError && [400,401,403,409,422,426].includes(error.status)) pending.current = null;
      setUnknown(Boolean(pending.current)); setIssue(error instanceof Error ? error.message : '程序登记尚未确认。');
    } finally { running.current = false; setBusy(false); setPassword(''); }
  }
  const locked = disabled || busy || unknown;
  return <details><summary>管理 MCP 程序</summary>
    <p className="muted">从本机已提供的程序中选择。登记程序后，可在下方配置工具范围。</p>
    <form onSubmit={(event) => { event.preventDefault(); const selected = catalog.choices.find((c) => c.source_ref === choice);
      void perform(selected && {kind:'register',source_ref:selected.source_ref,source_digest:selected.source_digest}); }}>
      <label>可登记程序<select value={choice} disabled={locked} onChange={(e) => setChoice(e.target.value)} required>
        <option value="">选择程序</option>{catalog.choices.map((c) => <option key={c.source_ref} value={c.source_ref}>{c.source_ref} · {c.scope}</option>)}
      </select></label>
      {catalog.choices.length === 0 && <p className="notice">当前安装尚未提供可登记的 MCP 程序。</p>}
      <label>确认登录密码<input type="password" autoComplete="current-password" maxLength={128} value={password} disabled={locked} onChange={(e) => setPassword(e.target.value)} required /></label>
      <button className="secondary-button" disabled={locked || !choice}>登记所选程序</button>
    </form>
    <ul className="service-list">{catalog.programs.map((p) => <li key={p.program_ref}><span>{p.source_ref} · {p.revoked ? '已撤销' : '已登记'}</span>
      {!p.revoked && <button className="secondary-button" disabled={locked || !password} onClick={() => void perform({kind:'revoke',program_ref:p.program_ref,registration_digest:p.registration_digest})}>撤销登记</button>}
    </li>)}</ul>
    {unknown && <button className="secondary-button" disabled={disabled || busy} onClick={() => void perform()}>核实原登记操作</button>}
    {issue && <p role="status" className="notice">{issue}</p>}
  </details>;
}
