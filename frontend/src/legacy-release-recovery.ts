import { isRecord } from './types.ts';

export const legacyEvidenceKinds = ['original_journal', 'archived_data_identity', 'updater_code_audit',
  'source_images_and_services', 'configuration_continuity', 'worker_continuity', 'management_reader_repair',
  'current_exclusive_inspection'] as const;
export interface LegacyReviewBinding {
  schema_version: 1; review_id: string; install_ref: string; owner: string; original_operation_id: string;
  original_command_digest: string; original_release_ref: string; original_release_digest: string;
  config_revision: number; config_digest: string; journal_seq: number; journal_head: string; expires_at: number;
}
export interface LegacyReviewEvidence {
  schema_version: 1; kind: typeof legacyEvidenceKinds[number]; artifact_digest: string;
  binding_digest: string; observation_digest: string;
}
export interface LegacyReleaseReview {
  schema_version: 1; binding: LegacyReviewBinding; evidence: LegacyReviewEvidence[];
  status: 'verified' | 'rejected' | 'unverifiable'; issues: string[];
}
export interface LegacyRecoveryCommand {
  schema_version: 1; operation_id: string; idempotency_key: string; expected_revision: number;
  review_id: string; review_digest: string; requested_deadline: number;
}
export type LegacyRecoveryRequest = LegacyRecoveryCommand & { owner: string };
export interface LegacyRecoveryView {
  schema_version: 1; request: LegacyRecoveryRequest; request_digest: string; revision: number;
  stage: 'intent' | 'committed'; review_digest: string; receipt_digest: string | null;
}
export interface LegacyRecoveryList { schema_version: 1; product_revision: number; owner: string; operations: LegacyRecoveryView[] }
export interface LegacyRecoveryVerification {
  schema_version: 1; operation_id: string; request_digest: string; expected_revision: number; requested_deadline: number;
}
const fail = () => new Error('历史审查记录无法核实，已停止操作。');
const positive = (value: unknown): value is number => Number.isSafeInteger(value) && Number(value) > 0;
const identifier = (value: unknown): value is string => typeof value === 'string' && /^[a-zA-Z0-9:._-]{1,128}$/.test(value) && value !== '.' && value !== '..';
const digest = (value: unknown): value is string => typeof value === 'string' && /^sha256:[a-f0-9]{64}$/.test(value);
function closed(value: unknown, fields: string[]): asserts value is Record<string, unknown> {
  if (!isRecord(value) || value.schema_version !== 1 || Object.keys(value).length !== fields.length
      || fields.some((key) => !Object.hasOwn(value, key))) throw fail();
}
export function parseLegacyReview(value: unknown, original?: string): LegacyReleaseReview {
  closed(value, ['schema_version', 'binding', 'evidence', 'status', 'issues']);
  const b = value.binding;
  closed(b, ['schema_version', 'review_id', 'install_ref', 'owner', 'original_operation_id', 'original_command_digest',
    'original_release_ref', 'original_release_digest', 'config_revision', 'config_digest', 'journal_seq', 'journal_head', 'expires_at']);
  if (![b.review_id, b.install_ref, b.owner, b.original_operation_id].every(identifier)
      || (original !== undefined && b.original_operation_id !== original)
      || ![b.original_command_digest, b.original_release_digest, b.config_digest, b.journal_head].every(digest)
      || ![b.config_revision, b.journal_seq, b.expires_at].every(positive)
      || !identifier(b.original_release_ref) || b.original_release_ref.includes(':')
      || !Array.isArray(value.evidence) || value.evidence.length > 8 || !Array.isArray(value.issues) || value.issues.length > 8
      || !value.issues.every(identifier) || new Set(value.issues).size !== value.issues.length
      || !['verified', 'rejected', 'unverifiable'].includes(String(value.status))) throw fail();
  let previous = -1;
  for (const item of value.evidence) {
    closed(item, ['schema_version', 'kind', 'artifact_digest', 'binding_digest', 'observation_digest']);
    const index = legacyEvidenceKinds.indexOf(item.kind as typeof legacyEvidenceKinds[number]);
    if (index <= previous || ![item.artifact_digest, item.binding_digest, item.observation_digest].every(digest)) throw fail();
    previous = index;
  }
  if (value.status === 'verified' ? value.evidence.length !== 8 || value.issues.length !== 0 : value.issues.length === 0) throw fail();
  return value as unknown as LegacyReleaseReview;
}
async function wireDigest(domain: string, value: unknown): Promise<string> {
  const result = await crypto.subtle.digest('SHA-256', new TextEncoder().encode(JSON.stringify([domain, value])));
  return `sha256:${Array.from(new Uint8Array(result), (byte) => byte.toString(16).padStart(2, '0')).join('')}`;
}
/** Exact Rust struct order, independent of HTTP JSON property order. Host remains the authority. */
export async function legacyReviewDigest(value: LegacyReleaseReview): Promise<string> {
  const r = parseLegacyReview(value), b = r.binding;
  const binding = { schema_version: b.schema_version, review_id: b.review_id, install_ref: b.install_ref, owner: b.owner,
    original_operation_id: b.original_operation_id, original_command_digest: b.original_command_digest,
    original_release_ref: b.original_release_ref, original_release_digest: b.original_release_digest,
    config_revision: b.config_revision, config_digest: b.config_digest, journal_seq: b.journal_seq, journal_head: b.journal_head, expires_at: b.expires_at };
  const bindingDigest = await wireDigest('forme-legacy-release-review-binding-v1', binding);
  if (r.evidence.some((item) => item.binding_digest !== bindingDigest)) throw fail();
  const evidence = r.evidence.map((e) => ({ schema_version: e.schema_version, kind: e.kind,
    artifact_digest: e.artifact_digest, binding_digest: e.binding_digest, observation_digest: e.observation_digest }));
  return wireDigest('forme-legacy-release-review-v1', { schema_version: r.schema_version, binding, evidence, status: r.status, issues: r.issues });
}
export function parseLegacyRecovery(value: unknown, operation?: string): LegacyRecoveryView {
  closed(value, ['schema_version', 'request', 'request_digest', 'revision', 'stage', 'review_digest', 'receipt_digest']);
  const r = value.request;
  closed(r, ['schema_version', 'operation_id', 'idempotency_key', 'expected_revision', 'owner', 'review_id', 'review_digest', 'requested_deadline']);
  if (![r.operation_id, r.idempotency_key, r.owner, r.review_id].every(identifier)
      || ![r.expected_revision, r.requested_deadline, value.revision].every(positive)
      || Number(value.revision) <= Number(r.expected_revision) || !digest(r.review_digest) || !digest(value.request_digest)
      || value.review_digest !== r.review_digest || (operation !== undefined && operation !== r.operation_id)
      || !['intent', 'committed'].includes(String(value.stage))
      || (value.stage === 'intent' ? value.receipt_digest !== null : !digest(value.receipt_digest))) throw fail();
  return value as unknown as LegacyRecoveryView;
}
export function parseLegacyRecoveries(value: unknown): LegacyRecoveryList {
  closed(value, ['schema_version', 'product_revision', 'owner', 'operations']);
  if (!positive(value.product_revision) || !identifier(value.owner) || !Array.isArray(value.operations) || value.operations.length > 25) throw fail();
  let previous = Number.MAX_SAFE_INTEGER;
  const ids = new Set<string>();
  for (const raw of value.operations) {
    const view = parseLegacyRecovery(raw);
    if (view.request.owner !== value.owner || view.revision > value.product_revision || view.revision >= previous || ids.has(view.request.operation_id)) throw fail();
    previous = view.revision; ids.add(view.request.operation_id);
  }
  return value as unknown as LegacyRecoveryList;
}
export async function verifyLegacyRecoveryDigest(view: LegacyRecoveryView): Promise<LegacyRecoveryView> {
  parseLegacyRecovery(view);
  const r = view.request;
  const request = { schema_version: r.schema_version, operation_id: r.operation_id, idempotency_key: r.idempotency_key,
    expected_revision: r.expected_revision, owner: r.owner, review_id: r.review_id, review_digest: r.review_digest, requested_deadline: r.requested_deadline };
  if (await wireDigest('forme-legacy-release-recovery-request-v1', request) !== view.request_digest) throw fail();
  return view;
}
export function legacyRecoveryCommand(review: LegacyReleaseReview, reviewDigest: string, operation: string, now: number): LegacyRecoveryCommand {
  parseLegacyReview(review);
  if (review.status !== 'verified' || !digest(reviewDigest) || !identifier(operation) || operation === review.binding.original_operation_id
      || !positive(now) || !positive(now + 120_000) || now >= review.binding.expires_at) throw fail();
  return { schema_version: 1, operation_id: operation, idempotency_key: operation, expected_revision: review.binding.journal_seq,
    review_id: review.binding.review_id, review_digest: reviewDigest, requested_deadline: Math.min(now + 120_000, review.binding.expires_at) };
}
export function legacyRecoveryVerification(view: LegacyRecoveryView, revision: number, now: number, review: LegacyReleaseReview): LegacyRecoveryVerification {
  parseLegacyRecovery(view);
  parseLegacyReview(review);
  const deadline = Math.min(now + 120_000, review.binding.expires_at);
  if (view.stage !== 'intent' || !positive(revision) || revision !== view.revision || !positive(now) || !positive(deadline) || deadline <= now
      || review.status !== 'verified' || review.binding.owner !== view.request.owner
      || review.binding.review_id !== view.request.review_id || review.binding.journal_seq !== view.request.expected_revision) throw fail();
  return { schema_version: 1, operation_id: view.request.operation_id, request_digest: view.request_digest,
    expected_revision: revision, requested_deadline: deadline };
}
