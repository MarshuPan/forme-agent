import { newKey } from '../api/client.ts';
import type { WorkbenchClient } from '../api/client.ts';
import type { FormEvent } from 'react';
import type { ProjectCommand, ProjectSupervisionCommand, WorkspaceProject, WorkspaceProjectControlView, RuntimeView } from '../types.ts';
import { useProjectCommand } from './project-command.ts';
import type { PendingChange } from './project-command.ts';

const labels = { Paused: '已暂停后续监管', ActiveRoute: '已有路线待收尾', Due: '等待主脑下次审查', NotDue: '尚未到复盘时机', Unverifiable: '当前无法完整判定' };
export function ProjectControls({ api, project, view, runtime, onChanged, onPending }: {
  api: WorkbenchClient; project: WorkspaceProject; view: WorkspaceProjectControlView | undefined; runtime: RuntimeView;
  onChanged: () => void; onPending: PendingChange;
}) {
  const supervision = useProjectCommand<ProjectSupervisionCommand>(`supervision:${project.reference}`, onPending);
  const revision = useProjectCommand<ProjectCommand>(`revision:${project.reference}`, onPending);
  async function toggle() {
    if (!view) return;
    const confirmed = await supervision.run(() => {
      const key = newKey('project-supervision');
      return { key, payload: { schema_version: 1, command: key, project: project.reference,
        expected_project_digest: view.project_digest, expected_version: view.supervision.version, enabled: !view.supervision.enabled } };
    }, async ({ payload }) => {
      const receipt = await api.projectSupervision(payload);
      if (receipt.command !== payload.command || receipt.status !== 'Confirmed') throw new Error('监管变更结果待核实，请保留原命令。');
    });
    if (confirmed) onChanged();
  }
  async function revise(event: FormEvent<HTMLFormElement>) {
    event.preventDefault(); if (!view) return;
    const form = new FormData(event.currentTarget);
    const confirmed = await revision.run(() => {
      const budget = Number(form.get('budget')), node = Number(form.get('node')), cadence = Number(form.get('cadence'));
      if (![budget, node, cadence].every((n) => Number.isSafeInteger(n) && n > 0) || node > budget) throw new Error('预算与周期必须为正整数，单节点不得超总预算。');
      const signature = { ...project.signature };
      for (const field of ['partition', 'dependency', 'verification', 'clarity', 'extent', 'reversibility'] as const) signature[field] = String(form.get(field));
      const key = newKey('project-revision');
      return { key, payload: { operation: 'revise', command: key, expected_digest: view.project_digest, project: {
        ...project, summary: String(form.get('summary')).trim(), model: String(form.get('model')),
        signature, budget_units: budget, node_units: node, cadence_ms: cadence * 60_000,
      } } };
    }, ({ key, payload }) => api.project(payload, key));
    if (confirmed) onChanged();
  }
  const signatureOptions = {
    partition: ['Whole', 'Sequential', 'Parallel', 'Iterative'], dependency: ['Independent', 'Interdependent'],
    verification: ['SelfCheck', 'Environment', 'IndependentReview'], clarity: ['Clear', 'Clarify', 'Explore'],
    extent: ['Single', 'MultiStage', 'LongRunning'], reversibility: ['LowRiskReversible', 'HighRiskIrreversible'],
  };
  return <section className="project-controls">
    <p role="status">监管：{view ? labels[view.review] : '正在读取权威状态'}</p>
    {view && <><p className="muted small">监管版本 {view.supervision.version} · 检查点 {view.checkpoint ? new Date(view.checkpoint.at).toLocaleString() : '暂无'} · 下次周期 {view.next_cadence === null ? '待首段完成' : new Date(view.next_cadence).toLocaleString()}</p>
      <p className="muted small">判定依据：{view.reasons.join('、')}{view.evidence.length > 0 ? ` · 证据 ${view.evidence.join('、')}` : ''}</p></>}
    <p className="muted small">暂停只阻止后续自动规划；当前路线继续保持原状态。恢复后等待主脑下一次受治理审查，不立即执行。</p>
    <button className="secondary-button" disabled={!view || supervision.operation.busy || supervision.operation.conflict} onClick={() => void toggle()}>
      {supervision.operation.pending ? '核实原监管变更' : view?.supervision.enabled ? '暂停后续监管' : '恢复后续监管'}
    </button>
    {supervision.error && <p className="form-error" role="alert">{supervision.error}</p>}
    {supervision.operation.conflict && <button className="text-button" onClick={() => { supervision.reloadConflict(); onChanged(); }}>重新读取监管状态</button>}
    <details><summary>调整后续项目段</summary><p className="muted small">目标、完成契约、工作空间和权限保持原绑定。这里的修改不会热换已有路线。</p>
      <form onSubmit={(event) => void revise(event)}><fieldset key={view?.project_digest} disabled={!view || revision.operation.busy || revision.operation.pending}>
        <label>项目说明<textarea name="summary" defaultValue={project.summary} required maxLength={8192} /></label>
        <label>模型<select name="model" defaultValue={project.model}>{Array.from(new Set([project.model, ...runtime.model_profiles])).map((model) => <option key={model}>{model}</option>)}</select></label>
        <div className="form-grid"><label>总预算<input name="budget" type="number" min="1" step="1" required defaultValue={project.budget_units} /></label><label>节点预算<input name="node" type="number" min="1" step="1" required defaultValue={project.node_units} /></label><label>周期（分钟）<input name="cadence" type="number" min="1" step="1" required defaultValue={Math.max(1, Math.round(project.cadence_ms / 60_000))} /></label></div>
        <div className="form-grid">{Object.entries(signatureOptions).map(([field, options]) => <label key={field}>{field}<select name={field} defaultValue={project.signature[field as keyof typeof signatureOptions]}>{options.map((option) => <option key={option}>{option}</option>)}</select></label>)}</div>
      </fieldset>
        {revision.error && <p className="form-error" role="alert">{revision.error}</p>}
        <button className="secondary-button" disabled={!view || revision.operation.busy || revision.operation.conflict}>{revision.operation.pending ? '核实原项目修订' : '保存后续项目调整'}</button>
        {revision.operation.conflict && <button type="button" className="text-button" onClick={() => { revision.reloadConflict(); onChanged(); }}>重新读取项目版本</button>}
      </form>
    </details>
  </section>;
}
