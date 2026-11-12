import { useEffect, useRef, useState } from 'react';
import { ApiError, newKey } from '../api/client.ts';
import type { WorkbenchClient } from '../api/client.ts';
import { mcpDigest, mcpDiscoveryRequest } from '../mcp-settings.ts';
import type { McpProgram, McpServer } from '../mcp-settings.ts';
import type { PendingApproval, RunSummary } from '../types.ts';
import { Approval } from './Approval.tsx';

const labels: Record<RunSummary['status'],string> = {
  Accepted:'已受理',Running:'执行中',Waiting:'等待处理',Complete:'已完成',Aborted:'已取消',Failed:'失败',Limited:'受限',
};
export function McpDiscovery({ api, owner, server, program, disabled }: {
  api: WorkbenchClient; owner: string; server: McpServer; program: McpProgram | undefined; disabled: boolean;
}) {
  const [runs,setRuns] = useState<RunSummary[]>([]), [approvals,setApprovals] = useState<PendingApproval[]>([]);
  const [busy,setBusy] = useState(false), [loading,setLoading] = useState(true), [issue,setIssue] = useState('');
  const [unknown,setUnknown] = useState<string | null>(null), [selected,setSelected] = useState('');
  const running = useRef(false), generation = useRef(0);
  async function refresh(expected?: string) {
    const current = ++generation.current;
    setLoading(true);
    try {
      const session = `session:mcp:${(await mcpDigest(server.server_ref)).slice(7)}`;
      if (expected) {
        const found = await api.run(expected);
        if (found.run !== expected || found.session !== session) throw new Error('原发现任务尚未确认。');
      }
      const [all, pending] = await Promise.all([api.runs(),api.approvals(session)]);
      if (generation.current !== current) return;
      const matching = all.filter((r) => r.session === session && r.run.startsWith('run:owner-mcp:'));
      setRuns(matching); setApprovals(pending.filter((a) => matching.some((r) => r.run === a.run)));
      if (expected) { setUnknown(null); setSelected(expected); }
    } catch (error) {
      if (generation.current === current) { setApprovals([]); setIssue(error instanceof Error ? error.message : '发现任务状态不可用。'); }
    } finally { if (generation.current === current) setLoading(false); }
  }
  useEffect(() => { if (!disabled) void refresh(); else { setLoading(false); setApprovals([]); }
    return () => { generation.current++; }; }, [api,owner,server.server_ref,disabled]);
  async function start() {
    if (!program || disabled || running.current || unknown || loading) return;
    running.current = true; setBusy(true); setIssue('');
    let expected: string | null = null;
    try {
      const original = await mcpDiscoveryRequest(owner,server,program,newKey('mcp-discovery'));
      expected = original.run; setUnknown(expected);
      await api.discoverMcp(original.request,expected);
      await refresh(expected);
    } catch (error) {
      if (error instanceof ApiError && [400,401,403,409,422,426].includes(error.status)) setUnknown(null);
      setIssue(error instanceof Error ? error.message : '发现任务结果尚未确认。');
    } finally { running.current = false; setBusy(false); }
  }
  async function verify(action: 'verify' | 'publish') {
    if (!selected || disabled || running.current) return;
    running.current = true; setBusy(true); setIssue('');
    try {
      await api.verifyMcpDiscovery(selected,action,newKey(`mcp-${action}`));
      setIssue(action === 'verify' ? '原任务核实已返回，请查看当前状态。' : '工具目录发布已确认，新任务可读取该目录。');
      await refresh(selected);
    } catch (error) { setIssue(error instanceof Error ? error.message : '原任务尚未核实。'); }
    finally { running.current = false; setBusy(false); }
  }
  const active = runs.some((r) => ['Accepted','Running','Waiting'].includes(r.status));
  return <details><summary>{server.server_ref} · 工具发现</summary>
    <p className="muted">发现会启动已登记的程序读取工具目录，需确认一次。任务执行结果不明时，可以核实原任务。</p>
    <div className="button-row"><button className="secondary-button" disabled={disabled || busy || loading || Boolean(unknown) || active || !server.enabled || !program || program.revoked} onClick={() => void start()}>发起工具发现</button>
      <button className="secondary-button" disabled={disabled || busy || loading} onClick={() => { setIssue(''); void refresh(unknown ?? undefined); }}>{unknown ? '核实原受理结果' : '刷新任务状态'}</button></div>
    {unknown && <p role="status" className="notice">发现任务尚未确认。核实操作只读取原任务，不重新启动程序。</p>}
    {runs.length > 0 && <><label>发现任务<select value={selected} disabled={disabled || busy || loading} onChange={(e) => setSelected(e.target.value)}>
      <option value="">选择原任务</option>{runs.map((r) => <option key={r.run} value={r.run}>{labels[r.status]} · {r.run}</option>)}
    </select></label><div className="button-row"><button className="secondary-button" disabled={disabled || busy || loading || !selected} onClick={() => void verify('verify')}>核实原执行结果</button>
      <button className="secondary-button" disabled={disabled || busy || loading || !selected} onClick={() => void verify('publish')}>发布已验证工具目录</button></div></>}
    {!disabled && approvals.map((a) => <Approval key={a.approval_id} item={a} api={api} changed={() => { void refresh(a.run); }} />)}
    {issue && <p role="status" className="notice">{issue}</p>}
  </details>;
}
