import { isRecord } from './types.ts';

export interface ProjectRouteNodeView {
  id: string;
  role: string;
  status: string;
  budget: number | null;
  attempts: number | null;
  consumed: number | null;
  reserved: number | null;
  summary: string;
  verification: string;
  evidence: string[];
  blockedBy: string[];
}

export interface ProjectRouteDependencyView {
  from: string;
  to: string;
  onFailure: string;
}

export interface ProjectRouteConflictView {
  kind: string;
  nodes: string[];
  reason: string;
  evidence: string[];
}

export interface ProjectRouteView {
  reference: string;
  project: string;
  status: string;
  pattern: string | null;
  confidence: number | null;
  budget: number | null;
  spent: number | null;
  remaining: number | null;
  closed: boolean;
  checkpointed: boolean;
  nodes: ProjectRouteNodeView[];
  dependencies: ProjectRouteDependencyView[];
  conflicts: ProjectRouteConflictView[];
}

const text = (value: unknown): string | null => typeof value === 'string' ? value : null;
const number = (value: unknown): number | null => typeof value === 'number' && Number.isFinite(value) ? value : null;
const strings = (value: unknown): string[] => Array.isArray(value) ? value.filter((item): item is string => typeof item === 'string') : [];
const isVersionOne = (value: unknown): value is Record<string, unknown> => isRecord(value) && value.schema_version === 1;

function verification(value: unknown): string {
  if (typeof value === 'string') return value;
  if (isRecord(value) && typeof value.Unverifiable === 'string') return `Unverifiable: ${value.Unverifiable}`;
  return 'Unknown';
}

export function projectRouteView(reference: string, raw: unknown): ProjectRouteView | null {
  if (!isVersionOne(raw) || !isVersionOne(raw.declaration)) return null;
  const declaration = raw.declaration;
  const project = text(declaration.project);
  const routeStatus = text(raw.status);
  const nodeRecords = isRecord(raw.nodes) ? raw.nodes : {};
  const rawNodes = Array.isArray(declaration.nodes) ? declaration.nodes : [];
  const declaredNodes = rawNodes.filter(isVersionOne);
  if (declaredNodes.length !== rawNodes.length) return null;
  const mainNode = isVersionOne(declaration.main) ? declaration.main : null;
  const specs = declaredNodes.length ? declaredNodes : mainNode ? [mainNode] : [];
  if (!project || !routeStatus || !specs.length) return null;

  const rawDependencies = Array.isArray(declaration.dependencies) ? declaration.dependencies : [];
  const dependencyRecords = rawDependencies.filter(isVersionOne);
  if (dependencyRecords.length !== rawDependencies.length) return null;
  const dependencies = dependencyRecords.flatMap((item) => {
    const from = text(item.from), to = text(item.to);
    if (!from || !to) return [];
    return [{ from, to, onFailure: text(item.on_failure) ?? 'Unknown' }];
  });
  const states = new Map<string, string>();
  for (const spec of specs) {
    const id = text(spec.id);
    if (!id) continue;
    const rawRecord = nodeRecords[id];
    if (!isVersionOne(rawRecord)) return null;
    const record = rawRecord;
    states.set(id, text(record?.status) ?? 'Unknown');
  }

  const nodes = specs.flatMap((spec): ProjectRouteNodeView[] => {
    const id = text(spec.id);
    if (!id) return [];
    const record = nodeRecords[id] as Record<string, unknown>;
    const status = text(record?.status) ?? 'Unknown';
    const blockedBy = status === 'Pending' ? dependencies
      .filter((edge) => edge.to === id)
      .filter((edge) => {
        const predecessor = states.get(edge.from) ?? 'Pending';
        return predecessor !== 'Done' && !(predecessor === 'Failed' && edge.onFailure === 'Continue');
      })
      .map((edge) => edge.from) : [];
    return [{
      id,
      role: text(spec.role) ?? 'Unknown',
      status,
      budget: number(spec.budget_units),
      attempts: number(record?.attempts),
      consumed: number(record?.consumed),
      reserved: number(record?.reserved),
      summary: text(record?.summary) ?? '',
      verification: verification(record?.verification),
      evidence: strings(record?.evidence),
      blockedBy,
    }];
  });

  const rawConflicts = Array.isArray(raw.conflicts) ? raw.conflicts : [];
  const conflictRecords = rawConflicts.filter(isVersionOne);
  if (conflictRecords.length !== rawConflicts.length) return null;
  const conflicts = conflictRecords.flatMap((item): ProjectRouteConflictView[] => {
    const kind = text(item.kind), reason = text(item.reason);
    if (!kind || !reason) return [];
    return [{ kind, reason, nodes: strings(item.nodes), evidence: strings(item.evidence) }];
  });

  return {
    reference: text(declaration.route) ?? reference,
    project,
    status: routeStatus,
    pattern: text(declaration.pattern),
    confidence: (() => {
      const value = number(declaration.confidence_millis);
      return value !== null && value >= 0 && value <= 1000 ? value : null;
    })(),
    budget: number(declaration.budget_units),
    spent: number(raw.spent),
    remaining: number(raw.remaining),
    closed: raw.closed === true,
    checkpointed: raw.checkpointed === true,
    nodes,
    dependencies,
    conflicts,
  };
}

export function routeStatusLabel(status: string, detail: ProjectRouteView | null = null): string {
  if (status === 'Running') {
    if (!detail) return '路线开放';
    if (detail.status !== status || detail.closed) return '状态待核实';
    if (detail.nodes.some((node) => node.status === 'OutcomeUnknown')) return '结果待核实';
    if (detail.nodes.some((node) => node.status === 'Running')) return '执行中';
    if (detail.nodes.some((node) => node.status === 'Waiting')) return '等待中';
    const unstarted = detail.spent === 0 && detail.nodes.length > 0 && detail.nodes.every((node) =>
      node.status === 'Pending' && node.attempts === 0 && node.consumed === 0 && node.reserved === 0);
    return unstarted ? '已规划，待执行' : '路线开放';
  }
  return {
    Completed: '已完成', Failed: '失败', Cancelled: '已取消',
    ReplanRequested: '需要重新规划', OutcomeUnknown: '结果待核实', Conflict: '存在冲突',
  }[status] ?? '状态未知';
}

export function nodeStatusLabel(status: string): string {
  return {
    Pending: '待开始', Running: '执行中', Waiting: '等待中', Done: '已完成',
    Failed: '失败', Cancelled: '已取消', OutcomeUnknown: '结果待核实',
  }[status] ?? '状态未知';
}

export function verificationLabel(value: string): string {
  if (value === 'Pass') return '验证通过';
  if (value === 'Fail') return '验证失败';
  if (value.startsWith('Unverifiable:')) return `无法验证（${value.slice('Unverifiable:'.length).trim()}）`;
  return '尚无验证结论';
}
