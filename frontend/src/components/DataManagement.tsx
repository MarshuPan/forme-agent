import { useEffect, useRef, useState } from 'react';
import type { FormEvent } from 'react';
import { ApiError, newKey } from '../api/client.ts';
import type { WorkbenchClient } from '../api/client.ts';
import { dataPage, dataState, prepareLifecycleAction } from '../data.ts';
import type { DataAction, DataCatalog, DataCommand, DataLifecycleSelection, DataOperations, DataReceipt, RestoreCandidate } from '../data.ts';
import type { MaterialLifecycleTarget } from '../memory-material.ts';
import { RestoreActivation } from './RestoreActivation.tsx';

function Receipt({ item, applicable, confirm, probe, activate, locked }: { item: DataReceipt; applicable: boolean; confirm: (item: DataReceipt) => void; probe: (key: string) => void; activate: (candidate: RestoreCandidate) => void; locked: boolean }) {
  return <article className="surface"><p className="eyebrow">{item.action} · {dataState[item.state]}</p>
    <p className="small muted">原操作：{item.command}</p>
    {item.state === 'Unknown' && <p className="notice">操作可能已经发生。这里只查询原回执，不重复执行。</p>}
    {item.backup && <dl><div><dt>数据对象</dt><dd>{item.backup.object}</dd></div><div><dt>事件数</dt><dd>{item.backup.events}</dd></div><div><dt>验证</dt><dd>{item.backup.verified ? '完整性、连续性与重放检查通过' : '已创建，尚未验证'}</dd></div></dl>}
    {item.plan && <><dl><div><dt>计划</dt><dd>{item.plan.operation}</dd></div><div><dt>范围</dt><dd>{item.plan.scope ?? `${item.plan.source} → ${item.plan.target}`}</dd></div><div><dt>涉及对象（包含派生项）</dt><dd>{item.plan.objects}</dd></div><div><dt>有效期</dt><dd>{new Date(item.plan.expires_at).toLocaleString()}</dd></div></dl>
      <p className={item.plan.irreversible ? 'notice' : 'muted'}>{item.plan.irreversible ? '永久清理可能不可逆，原始内容及其派生项会受影响。' : item.plan.operation === 'Retain' ? '该计划保留材料。' : '只恢复到新数据对象，当前主脑与外部动作保持原状。'}</p>
      <button type="button" className="secondary-button" disabled={!applicable || item.plan.expires_at <= Date.now()} onClick={() => confirm(item)}>{applicable ? '审阅并确认这个计划' : '该计划已有关联执行，请核实原回执'}</button></>}
    {item.lifecycle && <p>本地操作已确认：清理 {item.lifecycle.cleaned_projections} 类投影、销毁 {item.lifecycle.destroyed_keys} 个数据密钥。远端删除状态：{item.lifecycle.remote}。</p>}
    {item.state === 'Confirmed' && item.backup?.verified && item.backup.restoration && <button type="button" className="secondary-button" disabled={locked} onClick={() => activate(item.backup!.restoration!)}>查看主脑切换确认</button>}
    <button type="button" className="text-button" onClick={() => probe(item.command)}>查询原操作</button>
  </article>;
}

export function DataManagement({ api, connected, materialTarget, consumeMaterialTarget }: { api: WorkbenchClient; connected: boolean; materialTarget: MaterialLifecycleTarget | null; consumeMaterialTarget: () => void }) {
  const [catalog, setCatalog] = useState<DataCatalog | null>(null), [operations, setOperations] = useState<DataOperations | null>(null);
  const [busy, setBusy] = useState(false), [error, setError] = useState(''), [notice, setNotice] = useState('');
  const [target, setTarget] = useState(''), [source, setSource] = useState(''), [password, setPassword] = useState('');
  const [mode, setMode] = useState<'Backup' | 'PrepareRestore' | 'PrepareLifecycle' | 'Apply'>('Backup');
  const [selected, setSelected] = useState<DataLifecycleSelection | null>(null), [operation, setOperation] = useState<'Retain' | 'Delete' | 'CryptoShred'>('Delete');
  const [prepared, setPrepared] = useState<DataReceipt | null>(null), [confirmed, setConfirmed] = useState(false);
  const [current, setCurrent] = useState<DataReceipt | null>(null);
  const [restoration, setRestoration] = useState<RestoreCandidate | null>(null), [restoreLocked, setRestoreLocked] = useState(false);
  const pending = useRef<DataCommand | null>(null);
  const heading = useRef<HTMLHeadingElement>(null);

  useEffect(() => {
    if (!materialTarget) return;
    setSelected(materialTarget); setMode('PrepareLifecycle'); setOperation('Delete');
    setPrepared(null); setConfirmed(false); setPassword(''); setError(''); setNotice('');
    consumeMaterialTarget(); heading.current?.focus();
  }, [materialTarget, consumeMaterialTarget]);

  async function refresh() {
    if (!connected) return;
    const [subjects, history] = await Promise.all([api.queryData({ Catalog: { page: dataPage() } }), api.queryData({ Operations: { page: dataPage() } })]);
    if (subjects.kind === 'Catalog') setCatalog(subjects);
    if (history.kind === 'Operations') setOperations(history);
  }
  useEffect(() => { let active = true; setCatalog(null); setOperations(null);
    if (connected) void refresh().catch((problem: unknown) => { if (active) setError(problem instanceof Error ? problem.message : '数据状态读取失败。'); });
    return () => { active = false; };
  }, [api, connected]);

  async function probe(key: string) {
    setBusy(true); setError('');
    try {
      const result = await api.queryData({ Inspect: { command: key } });
      if (result.kind === 'Receipt') {
        setCurrent(result.receipt);
        if (result.receipt && result.receipt.state !== 'Unknown') pending.current = null;
        if (!result.receipt) setNotice('主脑尚无该操作的接收回执；请检查连接，不会自动重新发送。');
      }
      await refresh();
    } catch (problem) { setError(problem instanceof Error ? problem.message : '原操作回执暂不可用。'); }
    finally { setBusy(false); }
  }
  async function submit(event: FormEvent) {
    event.preventDefault(); if (busy || !connected || pending.current) return;
    setBusy(true); setError(''); setNotice('');
    try {
      let action: DataAction;
      if (mode === 'Backup') action = { Backup: { target } };
      else if (mode === 'PrepareRestore') action = { PrepareRestore: { source, target } };
      else if (mode === 'PrepareLifecycle') {
        action = prepareLifecycleAction(selected, catalog, operation);
      } else {
        if (!prepared?.plan || !confirmed) throw new Error('请明确确认已阅读计划范围与不可逆性。');
        action = { Apply: { prepared_by: prepared.command, plan_digest: prepared.plan.digest } };
      }
      const request: DataCommand = { schema_version: 1, command: newKey('data'), action };
      pending.current = request;
      const receipt = await api.commandData(request, password); setCurrent(receipt);
      if (receipt.state !== 'Unknown') pending.current = null;
      setConfirmed(false); await refresh();
    } catch (problem) {
      // These explicit rejection responses occur before a data action starts.
      // Transport/5xx outcomes retain the original key for read-only probing.
      if (problem instanceof ApiError && [400, 403, 409, 422, 429].includes(problem.status)) pending.current = null;
      setError(problem instanceof Error ? problem.message : '操作结果尚未确认，请查询原操作。');
    }
    finally { setPassword(''); setBusy(false); }
  }
  async function verify() {
    setBusy(true); setError('');
    try {
      if (connected) {
        const result = await api.queryData({ VerifyBackup: { source } });
        if (result.kind === 'Verification') setNotice(result.verified ? `备份 ${result.object} 的完整性、事件连续性与重放检查通过；没有导入或切换主脑。` : '备份尚未通过完整验证。');
      } else {
        const configuration = await api.productConfig();
        const result = await api.verifyOfflineBackup(source, configuration.active_revision);
        setNotice(`备份 ${result.verification.object} 的离线核验通过；没有启动、导入或切换主脑。`);
      }
    } catch (problem) { setError(problem instanceof Error ? problem.message : '备份验证失败。'); }
    finally { setBusy(false); }
  }
  async function more(kind: 'Catalog' | 'Operations') {
    const view = kind === 'Catalog' ? catalog : operations; if (!view || view.next_offset === null) return;
    setBusy(true); setError('');
    try {
      const query = { page: dataPage(view.next_offset, view.digest) };
      const result = await api.queryData(kind === 'Catalog' ? { Catalog: query } : { Operations: query });
      if (result.kind === 'Catalog') setCatalog((previous) => ({ ...result, subjects: [...(previous?.subjects ?? []), ...result.subjects] }));
      if (result.kind === 'Operations') setOperations((previous) => ({ ...result, operations: [...(previous?.operations ?? []), ...result.operations] }));
    } catch (problem) { setError(problem instanceof Error ? problem.message : '快照改变，请刷新后重读。'); }
    finally { setBusy(false); }
  }
  function selectPlan(item: DataReceipt) { setPrepared(item); setMode('Apply'); setConfirmed(false); }
  const applicable = (item: DataReceipt) => !operations?.operations.some((other) => other.prepared_by === item.command);

  return <section aria-label="数据管理"><header className="view-header"><h2 ref={heading} tabIndex={-1}>数据、备份与遗忘</h2><button className="secondary-button" disabled={busy || !connected} onClick={() => void refresh().catch((problem: unknown) => setError(problem instanceof Error ? problem.message : '刷新失败。'))}>刷新数据状态</button></header>
    {!connected && <p className="notice">主脑离线，数据更改暂不可用。你可以单独核验当前安装下的已有备份；这不会启动主脑或恢复数据。</p>}
    <p className="muted">归档与永久删除不同。恢复只建立新的数据对象；切换主脑需要独立维护流程。</p>
    {selected?.lifecycle_version !== undefined && <p className="notice">已从记忆核对来源范围，尚未执行清理。该范围可能含多条来源及派生对象，请先生成预览，再核对正式计划。若数据版本改变，须重新选择来源。</p>}
    {catalog?.pending_cleanup && <p className="notice">存在未完成清理，主脑数据读取已受保护性阻断。请核实原操作，不重复删除。</p>}
    {error && <p role="alert" className="form-error">{error}</p>}{notice && <p role="status">{notice}</p>}
    <div className="content-grid">{catalog?.subjects.map((subject) => <article className="surface" key={`${subject.scope}:${subject.subject}`}><h3>{subject.scope}</h3><p>{subject.objects} 个记录 / 已删除 {subject.deleted_objects} · {subject.state}</p><p className="muted small">投影：{subject.projections.join('、')}</p><details><summary>范围引用</summary><p>{subject.subject}</p></details><button type="button" className="secondary-button" disabled={busy || Boolean(pending.current) || subject.deleted_objects === subject.objects} onClick={() => { setSelected(subject); setMode('PrepareLifecycle'); setConfirmed(false); }}>准备保留或清理计划</button></article>)}</div>
    {catalog?.next_offset !== null && catalog && <button className="secondary-button" disabled={busy} onClick={() => void more('Catalog')}>读取更多材料范围</button>}
    <form className="surface" onSubmit={submit} aria-busy={busy}><fieldset disabled={busy || !connected || pending.current !== null}><legend>数据维护</legend>
      <label>操作<select value={mode} onChange={(event) => { setMode(event.target.value as typeof mode); setConfirmed(false); }}><option value="Backup">创建加密备份</option><option value="PrepareRestore">准备恢复到新对象</option>{selected && <option value="PrepareLifecycle">准备材料生命周期计划</option>}{prepared && <option value="Apply">批准已审阅计划</option>}</select></label>
      {(mode === 'Backup' || mode === 'PrepareRestore') && <label>新对象名称<input value={target} onChange={(event) => setTarget(event.target.value)} placeholder="my-backup.sqlite" required maxLength={96} /></label>}
      {mode === 'PrepareRestore' && <label>已验证备份名称<input value={source} onChange={(event) => setSource(event.target.value)} required maxLength={96} /></label>}
      {mode === 'PrepareLifecycle' && <><p>所选范围：{selected?.scope}</p><label>计划类型<select value={operation} onChange={(event) => setOperation(event.target.value as typeof operation)}><option value="Retain">保留</option><option value="Delete">删除内容与派生项</option><option value="CryptoShred">销毁密钥并清理派生项</option></select></label><p className="muted small">本步只准备预览，之后需要再次确认原计划才会执行。</p></>}
      {mode === 'Apply' && <><p>{prepared?.plan?.operation} · {prepared?.plan?.scope ?? `${prepared?.plan?.source} → ${prepared?.plan?.target}`} · {prepared?.plan?.objects} 个对象</p><label className="checkbox-label"><input type="checkbox" checked={confirmed} onChange={(event) => setConfirmed(event.target.checked)} required />我已核对该计划的范围与不可逆性，批准这一次操作。</label></>}
      <label>当前登录密码<input type="password" value={password} onChange={(event) => setPassword(event.target.value)} autoComplete="current-password" required maxLength={128} /></label>
      <button className="primary-button" disabled={busy}>{mode === 'Apply' ? '批准并执行原计划' : mode === 'Backup' ? '创建备份' : '生成安全预览'}</button>
    </fieldset>{pending.current && <p className="notice">原操作 {pending.current.command} 尚未确认。<button type="button" className="text-button" disabled={busy} onClick={() => void probe(pending.current!.command)}>只查询原回执</button></p>}</form>
    <div className="surface"><h3>只读验证已有备份</h3><label>备份名称<input value={source} maxLength={96} onChange={(event) => setSource(event.target.value)} /></label><button type="button" className="secondary-button" disabled={busy || !source} onClick={() => void verify()}>{connected ? '验证完整性与重放' : '离线核验已有备份'}</button></div>
    {restoration && <RestoreActivation api={api} candidate={restoration} lock={setRestoreLocked} />}
    {current && <Receipt item={current} applicable={applicable(current)} confirm={selectPlan} probe={(key) => void probe(key)} activate={setRestoration} locked={restoreLocked} />}
    <h3>原操作与回执</h3><div className="content-grid">{operations?.operations.map((item) => <Receipt key={item.command} item={item} applicable={applicable(item)} confirm={selectPlan} probe={(key) => void probe(key)} activate={setRestoration} locked={restoreLocked} />)}</div>
    {operations?.next_offset !== null && operations && <button className="secondary-button" disabled={busy} onClick={() => void more('Operations')}>读取更多操作</button>}
  </section>;
}
