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
