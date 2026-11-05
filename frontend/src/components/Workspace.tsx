import { useCallback, useEffect, useState } from 'react';
import type { FormEvent } from 'react';
import { newKey } from '../api/client.ts';
import type { WorkbenchClient } from '../api/client.ts';
import { isRecord } from '../types.ts';
import type { OrchestrationLedger, ProjectCommand, RuntimeView, ScheduledJob, TaskSignature, WorkspaceCharterRecord, WorkspaceCharterChangedPayload, WorkspaceProject, WorkspaceControlSnapshot, WorkspaceProjectControlView } from '../types.ts';
import { nodeStatusLabel, projectRouteView, routeStatusLabel, verificationLabel } from '../project-routes.ts';
import { ProjectControls } from './ProjectControls.tsx';
import { WorkspaceIntentions } from './WorkspaceIntentions.tsx';
import { useProjectCommand } from './project-command.ts';
import type { PendingChange } from './project-command.ts';
import { charterRulesError } from '../workspace-charter.ts';

const defaultSignature: TaskSignature = {
  schema_version: 1,
  partition: 'Whole',
  dependency: 'Independent',
  verification: 'IndependentReview',
  clarity: 'Clear',
  extent: 'Single',
  reversibility: 'LowRiskReversible',
  anchors: [],
};

const lines = (value: string) => value.split('\n').map((item) => item.trim()).filter(Boolean);
const displayLines = (value: string[]) => value.join('\n');
const inputDigest = async (value: string) => {
  const bytes = await crypto.subtle.digest('SHA-256', new TextEncoder().encode(value));
  return `sha256:${Array.from(new Uint8Array(bytes), (byte) => byte.toString(16).padStart(2, '0')).join('')}`;
};

function useWorkspaceData(api: WorkbenchClient, workspace: string, revision: number) {
  const [ledger, setLedger] = useState<OrchestrationLedger | null>(null);
  const [status, setStatus] = useState<WorkspaceControlSnapshot | null>(null);
  const [jobs, setJobs] = useState<ScheduledJob[]>([]);
  const [charter, setCharter] = useState<WorkspaceCharterRecord | null>(null);
  const [error, setError] = useState('');
  const [loading, setLoading] = useState(true);
  useEffect(() => {
    let active = true;
    if (!workspace) { setLoading(false); return () => { active = false; }; }
    setLoading(true); setError('');
    void Promise.all([api.projects(), api.projectStatus(), api.charter(workspace), api.jobs()]).then(([nextLedger, nextStatus, nextCharter, nextJobs]) => {
      if (nextLedger.revision !== nextStatus.revision) throw new Error('项目状态在读取期间已变化，请刷新后再操作。');
      if (active) { setLedger(nextLedger); setStatus(nextStatus); setJobs(nextJobs); setCharter(nextCharter); setLoading(false); }
    }).catch((problem: unknown) => {
      if (active) { setError(problem instanceof Error ? problem.message : '工作空间状态读取失败。'); setLoading(false); }
    });
    return () => { active = false; };
  }, [api, workspace, revision]);
  return { ledger, status, jobs, charter, error, loading };
}

function ProjectForm({ api, runtime, onChanged, onPending }: { api: WorkbenchClient; runtime: RuntimeView; onChanged: () => void; onPending: PendingChange }) {
  const mutation = useProjectCommand<ProjectCommand>(`register:${runtime.workspace}`, onPending);
  const { busy, pending, conflict } = mutation.operation;
  const [validation, setValidation] = useState('');
  const error = mutation.error || validation;
  const [signature, setSignature] = useState<TaskSignature>(defaultSignature);
  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault(); if (busy) return;
    const element = event.currentTarget;
    if (mutation.operation.pending) {
      const confirmed = await mutation.run(() => { throw new Error('应核实原登记命令。'); }, ({ key, payload }) => api.project(payload, key));
      if (confirmed) { element.reset(); onChanged(); }
      return;
    }
    const form = new FormData(element);
    const raw = String(form.get('reference') ?? '').trim().replace(/[^a-zA-Z0-9_-]+/g, '-');
    const reference = `project:${raw}`;
    const command = newKey('project-register');
    const budgetUnits = Math.floor(Number(form.get('budget') ?? 1));
    const nodeUnits = Math.floor(Number(form.get('node_budget') ?? 1));
    if (!Number.isFinite(budgetUnits) || !Number.isFinite(nodeUnits) || budgetUnits < 1 || nodeUnits < 1) {
      setValidation('预算必须是正整数。');
      return;
    }
    if (nodeUnits > budgetUnits) {
      setValidation('单节点预算不能大于项目总预算。');
      return;
    }
    const project: WorkspaceProject = {
      schema_version: 1,
      reference,
      workspace: runtime.workspace,
      goal: `goal:${raw}`,
      done: `done:${raw}`,
      summary: String(form.get('summary') ?? '').trim(),
      signature,
      requirements: ['ActionSucceeded'],
      scope: runtime.workspace,
      capabilities: [],
      model: String(form.get('model') ?? runtime.model_profiles[0] ?? ''),
      budget_units: budgetUnits,
      node_units: nodeUnits,
      cadence_ms: Math.max(1, Number(form.get('cadence_minutes') ?? 60)) * 60_000,
      deadline: null,
    };
    setValidation('');
    const confirmed = await mutation.run(() => ({ key: command, payload: { operation: 'register', command, project } }), ({ key, payload }) => api.project(payload, key));
    if (confirmed) { element.reset(); onChanged(); }
  }
  return <details className="surface project-create"><summary>登记一个长期项目</summary>
    <p className="muted small">项目登记只建立主脑的长期监管边界，不会自动创建子代理或立即执行动作。</p>
    <form onSubmit={(event) => void submit(event)}><fieldset disabled={busy || pending}>
      <label>项目标识<input name="reference" pattern="[a-zA-Z0-9_-]+" maxLength={64} placeholder="例如 stock-review" required /></label>
      <label>项目目标与说明<textarea name="summary" maxLength={8192} required placeholder="主脑需要长期关注什么，完成时应达到什么状态？" /></label>
      <label>使用模型<select name="model" defaultValue={runtime.model_profiles[0]}>{runtime.model_profiles.map((model) => <option key={model} value={model}>{model}</option>)}</select></label>
      <div className="form-grid"><label>总预算单位<input name="budget" type="number" min="1" defaultValue="8" required /></label><label>单节点单位<input name="node_budget" type="number" min="1" defaultValue="2" required /></label><label>监管周期（分钟）<input name="cadence_minutes" type="number" min="1" defaultValue="60" required /></label></div>
      <div className="form-grid">
        <label>任务拆分<select value={signature.partition} onChange={(event) => setSignature({ ...signature, partition: event.target.value })}><option>Whole</option><option>Sequential</option><option>Parallel</option><option>Iterative</option></select></label>
        <label>依赖关系<select value={signature.dependency} onChange={(event) => setSignature({ ...signature, dependency: event.target.value })}><option>Independent</option><option>Interdependent</option></select></label>
        <label>验证方式<select value={signature.verification} onChange={(event) => setSignature({ ...signature, verification: event.target.value })}><option>SelfCheck</option><option>Environment</option><option>IndependentReview</option></select></label>
        <label>任务清晰度<select value={signature.clarity} onChange={(event) => setSignature({ ...signature, clarity: event.target.value })}><option>Clear</option><option>Clarify</option><option>Explore</option></select></label>
        <label>持续范围<select value={signature.extent} onChange={(event) => setSignature({ ...signature, extent: event.target.value })}><option>Single</option><option>MultiStage</option><option>LongRunning</option></select></label>
        <label>可逆性<select value={signature.reversibility} onChange={(event) => setSignature({ ...signature, reversibility: event.target.value })}><option>LowRiskReversible</option><option>HighRiskIrreversible</option></select></label>
      </div>
      </fieldset>{error && <p className="form-error" role="alert">{error}</p>}
      <button className="primary-button" disabled={busy || conflict}>{busy ? '正在登记…' : pending ? '核实原登记命令' : '登记项目'}</button>
      {conflict && <button type="button" className="text-button" onClick={() => { mutation.reloadConflict(); onChanged(); }}>重新读取项目</button>}
    </form>
  </details>;
}

function CharterEditor({ api, runtime, charter, onChanged, onPending }: { api: WorkbenchClient; runtime: RuntimeView; charter: WorkspaceCharterRecord | null; onChanged: () => void; onPending: PendingChange }) {
  const mutation = useProjectCommand<WorkspaceCharterChangedPayload>(`charter:${runtime.workspace}`, onPending);
  const { busy, pending, conflict } = mutation.operation;
  const [inputError, setInputError] = useState('');
  const error = inputError || mutation.error;
  const base = charter ?? { schema_version: 1 as const, workspace: runtime.workspace, version: 0, goals: [], constraints: [], prohibitions: [], done_contract: null, review_cadence: null, actor: 'Owner', digest: 'sha256:pending' };
  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault(); if (busy) return;
    const form = new FormData(event.currentTarget);
    const record: WorkspaceCharterRecord = {
      ...base,
      goals: lines(String(form.get('goals') ?? '')),
      constraints: lines(String(form.get('constraints') ?? '')),
      prohibitions: lines(String(form.get('prohibitions') ?? '')),
      done_contract: String(form.get('done_contract') ?? '').trim() || null,
      review_cadence: Math.max(1, Number(form.get('cadence_minutes') ?? 60)) * 60_000,
    };
    if (!pending) {
      const problem = charterRulesError(runtime.workspace, record.constraints, record.prohibitions);
      setInputError(problem);
      if (problem) return;
    }
    const confirmed = await mutation.run(async () => {
      const key = newKey('charter');
      return { key, payload: await api.prepareCharter(runtime.workspace, record, base.version, key) };
    }, ({ key, payload }) => api.updateCharter(runtime.workspace, payload, key));
    if (confirmed) onChanged();
  }
  return <details className="surface" open={!charter}><summary>主脑对这个工作空间的约束</summary>
    <p className="muted small">编辑会生成新的版本和 canonical digest；进行中的路线不会被热修改，只影响后续协调。</p>
    <form onSubmit={(event) => void submit(event)} onChange={() => setInputError('')}><fieldset key={base.digest} disabled={busy || pending}>
      <p className="muted small" id="charter-reference-help">目标与完成标准填写当前工作空间已登记的引用，可先留空。项目的自然语言目标与说明在下方“登记一个长期项目”中填写。</p>
      <label>目标引用（每行一项，可留空）<textarea name="goals" defaultValue={displayLines(base.goals)} maxLength={8192} aria-describedby="charter-reference-help" placeholder="例如 goal:已登记项目" /></label>
      <p className="muted small" id="charter-rules-help">至少一项可执行规则，每行一项：read-only（只读）、require-review（路线需复核）、deny-capability:能力引用、allow-capability:能力引用、scope:当前工作空间内的范围、max-nodes:1至32。最终仍受主脑权限检查约束。</p>
      <label>约束（每行一项）<textarea name="constraints" defaultValue={displayLines(base.constraints)} maxLength={8192} aria-describedby="charter-rules-help" placeholder={'read-only\nrequire-review'} /></label>
      <p className="muted small" id="charter-prohibitions-help">禁止规则仅支持 read-only 或 deny-capability:能力引用；自然语言不会被当作已执行的限制。</p>
      <label>禁止规则（每行一项，可留空）<textarea name="prohibitions" defaultValue={displayLines(base.prohibitions)} maxLength={8192} aria-describedby="charter-prohibitions-help" placeholder="deny-capability:能力引用" /></label>
      <label>完成标准引用（可留空）<input name="done_contract" defaultValue={base.done_contract ?? ''} maxLength={512} aria-describedby="charter-reference-help" /></label>
      <label>复盘周期（分钟）<input name="cadence_minutes" type="number" min="1" defaultValue={Math.max(1, Math.round((base.review_cadence ?? 3_600_000) / 60_000))} required /></label>
      </fieldset>{error && <p className="form-error" role="alert">{error}</p>}
      <button className="primary-button" disabled={busy || conflict}>{busy ? '正在保存…' : pending ? '核实原约束保存命令' : '保存工作空间约束'}</button>
      {conflict && <button type="button" className="text-button" onClick={() => { mutation.reloadConflict(); onChanged(); }}>重新读取约束</button>}
    </form>
  </details>;
}

function ProjectCard({ api, project, routes, runtime, view, onChanged, onPending }: { api: WorkbenchClient; project: WorkspaceProject; routes: Array<[string, unknown]>; runtime: RuntimeView; view: WorkspaceProjectControlView | undefined; onChanged: () => void; onPending: PendingChange }) {
  const [input, setInput] = useState(project.summary);
  const mutation = useProjectCommand<ProjectCommand>(`route:${project.reference}`, onPending);
  const { busy, pending, conflict } = mutation.operation;
  const error = mutation.error;
  async function send(command?: ProjectCommand, key?: string) {
    const confirmed = await mutation.run(async () => {
      if (command && key) return { key, payload: command };
      const nextKey = newKey('route-plan');
      return { key: nextKey, payload: { operation: 'plan', command: nextKey, project: project.reference,
        signature: project.signature, input_digest: await inputDigest(input.trim() || project.summary) } };
    }, ({ key: originalKey, payload }) => api.project(payload, originalKey));
    if (confirmed) onChanged();
  }
  return <article className="surface project-card"><div className="project-heading"><div><p className="eyebrow">{project.reference}</p><h2>{project.summary}</h2></div><span className="status-pill">{project.model}</span></div>
    <dl><div><dt>工作空间</dt><dd>{project.workspace}</dd></div><div><dt>预算与监管</dt><dd>{project.budget_units} 单位 · {Math.round(project.cadence_ms / 60_000)} 分钟</dd></div><div><dt>任务签名</dt><dd>{project.signature.partition} / {project.signature.extent} / {project.signature.verification}</dd></div></dl>
    <ProjectControls api={api} project={project} view={view} runtime={runtime} onChanged={onChanged} onPending={onPending} />
    <label>本次路线输入（用于绑定 input digest）<textarea disabled={busy || pending} value={input} onChange={(event) => setInput(event.target.value)} maxLength={8192} /></label>
    <div className="button-row"><button className="secondary-button" disabled={busy || conflict} onClick={() => void send()}>{pending ? '核实原路线命令' : '规划路线'}</button></div>
    {conflict && <button className="text-button" onClick={() => { mutation.reloadConflict(); onChanged(); }}>重新读取路线</button>}
    {routes.map(([routeKey, raw]) => {
      const route = isRecord(raw) ? raw : {};
      const declaration = isRecord(route.declaration) ? route.declaration : {};
      const status = typeof route.status === 'string' ? route.status : 'Unknown';
      const routeRef = typeof declaration.route === 'string' ? declaration.route : routeKey;
      const detail = projectRouteView(routeKey, raw);
      const executeKey = `route-execute:${routeRef}`;
      const cancelKey = `route-cancel:${routeRef}`;
      return <div className="route-entry" key={routeKey}><div className="route-row"><div><strong>{routeRef}</strong><span className="muted small">{routeStatusLabel(status)}</span></div><div className="button-row"><button className="secondary-button" disabled={busy || pending || status !== 'Running'} onClick={() => void send({ operation: 'execute', route: routeRef }, executeKey)}>执行路线</button><button className="text-button" disabled={busy || pending || status !== 'Running'} onClick={() => void send({ operation: 'cancel', command: cancelKey, route: routeRef }, cancelKey)}>取消路线</button></div></div>
        {detail && <details className="route-details"><summary>路线详情 · {detail.nodes.length} 个节点 · 预算 {detail.budget ?? '未知'} · 已用 {detail.spent ?? '未知'} · 剩余 {detail.remaining ?? '未知'}</summary>
          <p className="muted small">模式：{detail.pattern ?? '未指定'} · 置信度：{detail.confidence === null ? '未记录' : `${(detail.confidence / 10).toFixed(1)}%`} · 检查点：{detail.checkpointed ? '已记录' : '未记录'} · 路线：{detail.closed ? '已关闭' : '开放'}</p>
          <ol className="route-node-list">{detail.nodes.map((node) => <li className="route-node" key={node.id}><div className="route-node-heading"><strong>{node.id}</strong><span>{nodeStatusLabel(node.status)}</span></div>
            <p className="muted small">角色：{node.role}{node.attempts === null ? '' : ` · 尝试 ${node.attempts}`}{node.budget === null ? '' : ` · 预算 ${node.budget}`}{node.consumed === null || node.reserved === null ? '' : ` · 已用 ${node.consumed} / 预留 ${node.reserved}`}</p>
            {node.blockedBy.length > 0 && <p className="notice">等待前置节点：{node.blockedBy.join('、')}</p>}
            {node.summary && <p>{node.summary}</p>}
            <p className="small">{verificationLabel(node.verification)}</p>
            {node.evidence.length > 0 && <p className="muted small">证据：{node.evidence.join('、')}</p>}
          </li>)}</ol>
          {detail.dependencies.length > 0 && <section className="route-subsection"><h3>依赖关系</h3><ul>{detail.dependencies.map((edge, index) => <li key={`${edge.from}:${edge.to}:${index}`}>{edge.from} → {edge.to} · 失败时：{edge.onFailure}</li>)}</ul></section>}
          {detail.conflicts.length > 0 && <section className="route-subsection"><h3>冲突</h3><ul>{detail.conflicts.map((conflict, index) => <li key={`${conflict.kind}:${index}`}>{conflict.kind}：{conflict.reason} · 节点 {conflict.nodes.join('、') || '未指定'}{conflict.evidence.length ? ` · 证据 ${conflict.evidence.join('、')}` : ''}</li>)}</ul></section>}
        </details>}
      </div>;
    })}
    {error && <p className="form-error" role="alert">{error}</p>}
  </article>;
}

export function WorkspaceView({ api, runtime, open }: { api: WorkbenchClient; runtime: RuntimeView | null; open: (run: string) => void }) {
  const [revision, setRevision] = useState(0);
  const [workspace, setWorkspace] = useState(runtime?.workspace ?? '');
  const [pending, setPending] = useState<Record<string, boolean>>({});
  const onPending = useCallback((name: string, value: boolean) => setPending((current) => current[name] === value ? current : { ...current, [name]: value }), []);
  useEffect(() => { if (!workspace && runtime) setWorkspace(runtime.workspace); }, [workspace, runtime]);
  const { ledger, status, jobs, charter, error, loading } = useWorkspaceData(api, runtime ? workspace : '', revision);
  const refresh = useCallback(() => setRevision((value) => value + 1), []);
  const projects = ledger ? Object.values(ledger.projects).filter((p) => p.workspace === workspace) : [];
  const routes = ledger ? Object.entries(ledger.routes) : [];
  return <section className="view" aria-labelledby="workspace-title"><header className="view-header"><div><p className="eyebrow">长期工作的上下文</p><h1 id="workspace-title">工作空间</h1></div><button className="secondary-button" onClick={refresh}>刷新</button></header>
    {!runtime && <p className="notice">主脑服务不可用，工作空间命令暂时停用。</p>}
    {loading && <p className="loading" role="status">正在读取主脑的项目状态…</p>}
    {error && <p className="form-error" role="alert">{error}</p>}
    {runtime && <><label>当前工作空间<select value={workspace} disabled={Object.values(pending).some(Boolean) || loading} onChange={(event) => setWorkspace(event.target.value)}>{Array.from(new Set([runtime.workspace, ...(status?.workspaces ?? [])])).map((w) => <option key={w}>{w}</option>)}</select></label>
      {Object.values(pending).some(Boolean) && <p className="notice">有命令尚待核实，请保留当前页面并核实原操作后再切换工作空间。</p>}
      <fieldset className="workspace-body" disabled={loading || Boolean(error)}>
      <CharterEditor key={`charter:${workspace}`} api={api} runtime={{ ...runtime, workspace }} charter={charter?.workspace === workspace ? charter : null} onChanged={refresh} onPending={onPending} /><ProjectForm key={`register:${workspace}`} api={api} runtime={{ ...runtime, workspace }} onChanged={refresh} onPending={onPending} />
      {projects.length ? <div className="content-grid">{projects.map((project) => <ProjectCard key={project.reference} api={api} project={project} runtime={runtime} view={status?.projects.find((v) => v.project === project.reference)} routes={routes.filter(([, route]) => isRecord(route) && isRecord(route.declaration) && route.declaration.project === project.reference)} onChanged={refresh} onPending={onPending} />)}</div> : <div className="empty-state"><h2>还没有登记的长期项目</h2><p>登记项目后，主脑才会按工作空间约束持续监管。</p></div>}
      <WorkspaceIntentions api={api} jobs={jobs} workspace={workspace} onChanged={refresh} onPending={onPending} open={open} />
      </fieldset>
      {ledger && <details className="surface ledger-details"><summary>编排账本与版本</summary><p className="muted small">当前账本修订：{ledger.revision}。所有项目命令仍通过主脑事件写入和重放。</p><pre>{JSON.stringify(ledger, null, 2)}</pre></details>}
    </>}
  </section>;
}
