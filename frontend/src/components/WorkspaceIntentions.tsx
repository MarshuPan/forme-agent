import { isRecord } from '../types.ts';
import type { ScheduledJob } from '../types.ts';
import type { WorkbenchClient } from '../api/client.ts';
import { useProjectCommand } from './project-command.ts';
import type { PendingChange } from './project-command.ts';

export function workspaceJob(job: ScheduledJob, workspace: string): boolean {
  if (!isRecord(job.binding) || !isRecord(job.binding.envelope)) return false;
  const scope = job.binding.envelope.scope;
  return typeof scope === 'string' && (scope === workspace || scope.startsWith(`${workspace}/`));
}
function Intention({ api, job, onChanged, onPending, open }: { api: WorkbenchClient; job: ScheduledJob; onChanged: () => void; onPending: PendingChange; open: (run: string) => void }) {
  const intention = isRecord(job.intention) ? job.intention : {};
  const reference = typeof intention.id === 'string' ? intention.id : '';
  const command = useProjectCommand<string>(`intention:${reference}`, onPending);
  async function cancel() {
    const confirmed = await command.run(() => ({ key: `intention-cancel:${reference}`, payload: reference }), ({ key, payload }) => api.cancelJob(payload, key));
    if (confirmed) onChanged();
  }
  return <article className="route-entry"><strong>{reference || '无可验证意图标识'}</strong>
    <p>{String(intention.state ?? 'Unknown')}{job.run_status ? ` · 关联运行 ${job.run_status}` : ''}{job.manual_review ? ' · 需要人工核实' : ''}</p>
    <p className="muted small">取消意图只阻止后续触发，不能据此认定已启动动作停止；在途运行请使用其正式控制。</p>
    {job.run && <button className="text-button" onClick={() => open(job.run!)}>查看关联运行：{job.run}</button>}
    <button className="text-button" disabled={!reference || intention.state !== 'Pending' || command.operation.busy || command.operation.conflict} onClick={() => void cancel()}>{command.operation.pending ? '核实原取消命令' : '取消后续意图'}</button>
    {command.error && <p className="form-error" role="alert">{command.error}</p>}
    {command.operation.conflict && <button className="text-button" onClick={() => { command.reloadConflict(); onChanged(); }}>重新读取意图</button>}
  </article>;
}
export function WorkspaceIntentions({ api, jobs, workspace, onChanged, onPending, open }: {
  api: WorkbenchClient; jobs: ScheduledJob[]; workspace: string; onChanged: () => void; onPending: PendingChange; open: (run: string) => void;
}) {
  const relevant = jobs.filter((job) => workspaceJob(job, workspace));
  return <section className="surface"><h2>长期意图</h2>{relevant.length ? relevant.map((job, index) => <Intention key={isRecord(job.intention) && typeof job.intention.id === 'string' ? job.intention.id : index} api={api} job={job} onChanged={onChanged} onPending={onPending} open={open} />) : <p className="muted">当前没有绑定此工作空间的长期意图。</p>}</section>;
}
