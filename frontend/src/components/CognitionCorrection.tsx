import { useEffect, useRef, useState } from 'react';
import type { FormEvent } from 'react';
import { newKey } from '../api/client.ts';
import type { WorkbenchClient } from '../api/client.ts';
import { correctionCommand } from '../correction.ts';
import type { CognitionCorrection, CorrectionsView } from '../correction.ts';
import type { WorkbenchItem } from '../workbench.ts';

export function CognitionCorrectionForm({ api, item, snapshot, changed }: {
  api: WorkbenchClient; item: WorkbenchItem; snapshot: string; changed: () => void;
}) {
  const [statement, setStatement] = useState(''), [confirmed, setConfirmed] = useState(false);
  const [busy, setBusy] = useState(false), [message, setMessage] = useState('');
  const [pending, setPending] = useState<CognitionCorrection | null>(null);
  const active = useRef(true);
  useEffect(() => { active.current = true; return () => { active.current = false; }; }, []);
  async function submit(event: FormEvent) {
    event.preventDefault(); if (busy || pending || !confirmed) return;
    let command: CognitionCorrection;
    try { command = correctionCommand(item, snapshot, statement, newKey('correction')); }
    catch (problem) { setMessage(problem instanceof Error ? problem.message : '请核对纠正说明。'); return; }
    setPending(command); setBusy(true); setMessage('');
    try {
      await api.correctCognition(command);
      if (active.current) { setStatement(''); setMessage('反馈、原认知失效与新候选已记录。新说法尚未提升。'); changed(); }
    } catch (problem) {
      if (active.current) setMessage(problem instanceof Error ? problem.message : '结果尚未确认，请查询原命令。');
    } finally { if (active.current) setBusy(false); }
  }
  async function inspect() {
    if (busy || !pending) return;
    setBusy(true);
    try {
      const view = await api.corrections(pending.command);
      if (active.current) {
        if (view.receipts.length === 1) { setStatement(''); setMessage('已核实完整提交。纠正候选仍需治理审查。'); changed(); }
        else setMessage('尚未查到接纳回执。请保留原命令继续核实；这不代表可以更换命令重复提交。');
      }
    } catch (problem) { if (active.current) setMessage(problem instanceof Error ? problem.message : '原回执读取失败。'); }
    finally { if (active.current) setBusy(false); }
  }
  return <details><summary>用文字纠正这条认知</summary><form onSubmit={submit} aria-busy={busy}>
    <p>原对象将停用或降级，依赖对象进入重评。反馈只形成低置信候选，不能改变权限或直接覆盖稳定认知。</p>
    <p className="muted small">本次范围：{item.scope}；根对象及传递派生关系：{item.derived_count} 项。</p>
    <fieldset disabled={busy || pending !== null}><label>纠正说明<textarea required rows={4} value={statement} onChange={(event) => setStatement(event.target.value)} /></label>
      <p className="muted small">{new TextEncoder().encode(statement).length} / 8192 UTF-8 字节</p>
      <label className="checkbox-label"><input type="checkbox" checked={confirmed} onChange={(event) => setConfirmed(event.target.checked)} />我已核对原对象、影响范围与纠正文字</label>
    </fieldset>
    {pending ? <><p className="muted small">原命令：<code>{pending.command}</code>。核实只读取回执，不会再次纠正。</p>
      <button type="button" className="secondary-button" disabled={busy} onClick={() => void inspect()}>核实原纠正</button></>
      : <button className="secondary-button" disabled={busy || !confirmed}>确认并记录纠正</button>}
    {message && <p role="status">{message}</p>}
  </form></details>;
}

export function CorrectionHistory({ api, revision }: { api: WorkbenchClient; revision: string | undefined }) {
  const [view, setView] = useState<CorrectionsView | null>(null), [error, setError] = useState('');
  const [command, setCommand] = useState(''), [busy, setBusy] = useState(false);
  const active = useRef(true);
  useEffect(() => { active.current = true; return () => { active.current = false; }; }, []);
  useEffect(() => {
    let current = true; setView(null); setError('');
    void api.corrections().then((value) => { if (current) setView(value); })
      .catch((problem: unknown) => { if (current) setError(problem instanceof Error ? problem.message : '纠正历史读取失败。'); });
    return () => { current = false; };
  }, [api, revision]);
  async function inspect(event: FormEvent) {
    event.preventDefault(); if (busy || !command.trim()) return;
    setBusy(true); setError('');
    try { const result = await api.corrections(command); if (active.current) setView(result); }
    catch (problem) { if (active.current) setError(problem instanceof Error ? problem.message : '核实失败。'); }
    finally { if (active.current) setBusy(false); }
  }
  return <details><summary>纠正回执与原命令核实</summary>
    <form onSubmit={inspect}><label>原纠正命令<input value={command} maxLength={256} required disabled={busy} onChange={(event) => setCommand(event.target.value)} /></label>
      <button className="secondary-button" disabled={busy}>只读核实原命令</button></form>
    {error && <p role="alert" className="form-error">{error}</p>}
    {!view && !error && <p role="status">正在读取纠正回执…</p>}
    {view && <><p className="muted">回执证明反馈与失效已提交，不证明新候选已提升。{view.truncated ? '这里只显示最近 100 条，可输入原命令核实。' : ''}</p>
      {view.receipts.length === 0 && <p>未查到已接纳回执；这不是重新提交未知操作的依据。</p>}
      <ul>{view.receipts.map((receipt) => <li key={receipt.command}>{receipt.command} · 已完整记录 · 新候选 {receipt.candidate}</li>)}</ul></>}
  </details>;
}
