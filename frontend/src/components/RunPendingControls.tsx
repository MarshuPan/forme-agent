import { useRef, useState } from 'react';
import { newKey } from '../api/client.ts';
import type { WorkbenchClient } from '../api/client.ts';
import type { RunControlResolutionCommand, RunPendingKind, RunPendingView } from '../types.ts';

type Resolution = { command: string; run: string; body: unknown; kind: 'ToolInterrupt' | 'Handoff' };

export function RunPendingControls({ api, view, changed }: { api: WorkbenchClient; view: RunPendingView | null; changed: () => void }) {
  const [outcome, setOutcome] = useState<'Completed' | 'Failed' | 'Cancelled'>('Completed');
  const [reference, setReference] = useState('');
  const [summary, setSummary] = useState('');
  const [accepted, setAccepted] = useState(true);
  const [result, setResult] = useState('');
  const [operation, setOperation] = useState<Resolution | null>(null);
  const [busy, setBusy] = useState(false);
  const operationRef = useRef<Resolution | null>(null);
  const busyRef = useRef(false);
  const [error, setError] = useState('');
  const pending = view?.pending ?? null;
  const restoredOperation = pending?.kind === 'ResolutionUnknown' && pending.replay_command
    ? restore(pending.replay_command, view!.run)
    : null;
  const activeOperation = operation?.run === view?.run ? operation : restoredOperation;
  const locked = activeOperation !== null;

  function restore(command: RunControlResolutionCommand, run: string): Resolution {
    if ('ResolveToolOutcome' in command) return {
      command: command.ResolveToolOutcome.command, run, kind: 'ToolInterrupt', body: command,
    };
    return { command: command.ResolveHandoff.command, run, kind: 'Handoff', body: command };
  }

  function build(kind: RunPendingKind, command: string): Resolution | null {
    if (kind.kind === 'ToolInterrupt') return { command, run: view!.run, kind: 'ToolInterrupt', body: { ResolveToolOutcome: { command, resolution: {
      schema_version: 1, action: kind.action, outcome,
      result_ref: outcome === 'Completed' ? reference : null,
      failure_ref: outcome === 'Failed' ? reference : null,
      reason: outcome === 'Cancelled' ? reference : null,
      summary: outcome === 'Completed' || outcome === 'Failed' ? summary : '',
    } } } };
    if (kind.kind === 'Handoff') return { command, run: view!.run, kind: 'Handoff', body: { ResolveHandoff: { command, resolution: {
      schema_version: 1, target: kind.target, accepted, result: result || null,
    } } } };
    return null;
  }

  async function send(next: Resolution) {
    if (busyRef.current) return;
    busyRef.current = true;
    operationRef.current = next;
    setOperation(next); setBusy(true); setError('');
    try {
      await api.controlRun(next.run, next.body, next.command);
      operationRef.current = null;
      setOperation(null); setReference(''); setSummary(''); setResult(''); changed();
    } catch (problem) {
      setError(problem instanceof Error ? problem.message : '裁决结果尚未确认。保留原命令后再次核实。');
    } finally { busyRef.current = false; setBusy(false); }
  }

  async function resolve(kind: RunPendingKind) {
    const retained = operationRef.current ?? operation ?? restoredOperation;
    const next = retained?.run === view?.run ? retained : build(kind, newKey('owner-resolution'));
    if (next) await send(next);
  }

  if (operation && operation.run !== view?.run) return <section className="approval-card" role="status">
    <p className="eyebrow">另一项 owner 裁决尚未核实</p>
    <p>返回运行 {operation.run}，使用原命令核实；当前运行不能覆盖这项待确认操作。</p>
  </section>;
  if (!pending) return operation ? <section className="approval-card" role="status"><p className="eyebrow">裁决结果待核实</p><p>原操作 {operation.run} 的命令已保留。重新打开该运行以核实，不要创建新命令。</p></section> : null;
  return <section className="approval-card" aria-live="polite">
    {pending.kind === 'ToolInterrupt' && <>
      <p className="eyebrow">工具结果待你裁决</p>
      <p>副作用结果未知。请先核实外部结果；此处只记录你的确认，不是后台验证。</p>
      <label>裁决<select value={outcome} disabled={locked || busy} onChange={(e) => setOutcome(e.target.value as typeof outcome)}>
        <option value="Completed">确认已完成</option><option value="Failed">确认失败</option><option value="Cancelled">确认已取消</option>
      </select></label>
      <label>{outcome === 'Completed' ? '结果引用' : outcome === 'Failed' ? '失败证据引用' : '取消原因引用'}<input value={reference} disabled={locked || busy} maxLength={256} onChange={(e) => setReference(e.target.value)} required /></label>
      {(outcome === 'Completed' || outcome === 'Failed') && <label>说明<input value={summary} disabled={locked || busy} maxLength={4096} onChange={(e) => setSummary(e.target.value)} /></label>}
      <p className="muted small">检查点：{pending.checkpoint} · {pending.probe_hint}</p>
      <button className="primary-button" disabled={busy || (!locked && !reference.trim())} onClick={() => void resolve(pending)}>{busy ? '正在提交…' : locked ? '核实原裁决' : '记录我的裁决'}</button>
    </>}
    {pending.kind === 'Handoff' && <>
      <p className="eyebrow">等待协作结果</p><p>子代理不直接面向你；请确认协作结果后交回主脑。</p>
      <label className="inline-check"><input type="checkbox" checked={accepted} disabled={locked || busy} onChange={(e) => setAccepted(e.target.checked)} />协作已完成</label>
      <label>协作结果<textarea value={result} disabled={locked || busy} maxLength={16384} rows={3} onChange={(e) => setResult(e.target.value)} /></label>
      <p className="muted small">目标：{pending.target} · 检查点：{pending.checkpoint}</p>
      <button className="primary-button" disabled={busy} onClick={() => void resolve(pending)}>{busy ? '正在提交…' : locked ? '核实原协作裁决' : '提交协作结果'}</button>
    </>}
    {pending.kind === 'ApprovalWait' && <><p className="eyebrow">审批状态待恢复</p><p>审批：{pending.approval} · 检查点：{pending.checkpoint}</p></>}
    {pending.kind === 'ResolutionUnknown' && <>
      <p className="eyebrow">操作结果尚不确定</p>
      <p>命令可能已经生效。系统不会重做动作；请查看原事件，必要时请求停止运行。</p>
      <p className="muted small">检查点：{pending.checkpoint}{pending.command_digest ? ` · 命令摘要：${pending.command_digest}` : ''}</p>
      {(pending.reason === 'ClaimWithoutResult' || pending.reason === 'ActionWithoutRunTerminal') && activeOperation
        ? <button className="primary-button" disabled={busy} onClick={() => void send(activeOperation)}>{busy ? '正在核实…' : '用原命令核实'}</button>
        : <p>原裁决命令无法安全恢复。系统不会生成替代命令；请检查原事件并按安全处置流程处理。</p>}
    </>}
    {error && <p role="alert" className="form-error">{error}</p>}
  </section>;
}
