import type { Json } from './types.ts';

export type WorkbenchSection = 'Memory' | 'Cognition' | 'Inbox';
export type ReviewState = 'Candidate' | 'Promoted' | 'Rejected' | 'Downgraded' | 'Decayed' | 'Retracted';
export type ReviewDecision = 'Promote' | 'Reject' | 'Downgrade' | 'Retract';
export interface WorkbenchItem {
  schema_version: 1; id: string; kind: 'Memory' | 'Candidate' | 'Stable' | 'Reevaluation' | 'Suggestion' | 'AskToLearn' | 'Failure' | 'Waiting';
  run: string; event: string; stream_seq: number; at: number; scope: string | null;
  state: string; summary: string; content: string | null; tier: string | null; confidence: number | null;
  evidence: string[]; evidence_count: number; derived: string[]; derived_count: number; reviews: ReviewDecision[];
  learning: { schema_version: 1; goal: string; scope: string; target: Json; reason: string; impact: string; origin: string } | null;
}
export interface WorkbenchSnapshot {
  schema_version: 1; section: WorkbenchSection; digest: string; total: number;
  offset: number; next_offset: number | null; items: WorkbenchItem[];
}
export interface WorkbenchReview {
  schema_version: 1; command: string; run: string; candidate: string; expected_state: ReviewState; decision: ReviewDecision;
}
export interface WorkbenchProposal {
  schema_version: 1; command: string; run: string; proposal: string;
  outcome: 'Adopt' | 'Reject' | 'Defer'; defer_until: number | null;
}
export interface PendingWorkbenchProposals {
  schema_version: 1; commands: WorkbenchProposal[];
}
export interface WorkbenchReceipt {
  schema_version: 1; command: string; binding: string; status: 'Confirmed' | 'Rejected' | 'Unknown'; event: string;
}
export const reviewLabels: Record<ReviewDecision, string> = { Promote: '确认并提交提升审查', Reject: '拒绝候选', Downgrade: '降低稳定性', Retract: '撤回并重评派生对象' };
export const stateLabels: Record<string, string> = {
  Candidate: '待审查候选', Promoted: '已提升', Rejected: '已拒绝', Downgraded: '已降级', Decayed: '已衰减', Retracted: '已撤回',
  Active: '有效', Inactive: '已停用', Pending: '待处理', Reevaluated: '已重新评估', Recorded: '已记录',
  Answered: '已回答，等待认知审查', Reject: '已拒绝', Adopt: '已采纳', Defer: '已延后',
  ReviewUnknown: '处置结果待核实，已阻止重复操作',
};

const pendingRunLabels: Record<string, string> = {
  ApprovalWait: '等待 owner 处理审批',
  ToolInterrupt: '工具执行结果需要核实',
  Handoff: '等待协作结果',
  ResolutionUnknown: '运行结果待核实',
};

export function inboxItemStatus(item: Pick<WorkbenchItem, 'kind' | 'state'>): string {
  if (item.kind === 'Waiting') return pendingRunLabels[item.state] ?? '运行待决状态待核实';
  return stateLabels[item.state] ?? item.state;
}

export function isWorkbenchProposal(value: unknown): value is WorkbenchProposal {
  if (typeof value !== 'object' || value === null || Array.isArray(value)) return false;
  const command = value as Record<string, unknown>;
  const fields = ['schema_version', 'command', 'run', 'proposal', 'outcome', 'defer_until'];
  const validId = (id: unknown) => typeof id === 'string' && id.trim().length > 0
    && new TextEncoder().encode(id).length <= 256 && !/\p{Cc}/u.test(id);
  return Object.keys(command).length === fields.length && fields.every((field) => Object.hasOwn(command, field))
    && command.schema_version === 1 && validId(command.command) && validId(command.run) && validId(command.proposal)
    && (command.outcome === 'Defer'
      ? typeof command.defer_until === 'number' && Number.isSafeInteger(command.defer_until) && Number.isFinite(new Date(command.defer_until).getTime())
      : (command.outcome === 'Adopt' || command.outcome === 'Reject') && command.defer_until === null);
}

export function pendingProposalFor(
  item: Pick<WorkbenchItem, 'kind' | 'run' | 'id'>,
  commands: WorkbenchProposal[],
): WorkbenchProposal | undefined {
  if (item.kind !== 'Suggestion') return undefined;
  return commands.find((command) => command.run === item.run && command.proposal === item.id);
}
