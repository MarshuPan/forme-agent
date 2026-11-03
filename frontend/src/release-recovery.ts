import { isRecord } from './types.ts';

export interface ReleaseRecoveryCandidate {
  schema_version: 1; product_revision: number; owner: string; original_operation_id: string;
  original_command_digest: string; original_release_ref: string; original_release_digest: string;
  config_revision: number; config_digest: string; evidence_digest: string;
}
export interface ReleaseRecoveryCommand {
  schema_version: 1; operation_id: string; idempotency_key: string; expected_revision: number;
  decision: 'abort_before_switch'; candidate: ReleaseRecoveryCandidate; requested_deadline: number;
}
export type ReleaseRecoveryRequest = ReleaseRecoveryCommand & { owner: string };
export interface ReleaseRecoveryView {
  schema_version: 1; request: ReleaseRecoveryRequest; request_digest: string;
  revision: number; stage: 'intent' | 'committed'; receipt_digest: string | null;
}
export interface ReleaseRecoveryVerification {
  schema_version: 1; operation_id: string; request_digest: string; expected_revision: number; requested_deadline: number;
}
export interface ReleaseRecoveryList {
  schema_version: 1; product_revision: number; owner: string; operations: ReleaseRecoveryView[];
}
const fail = () => new Error('升级恢复记录无法核实，已停止操作。');
const positive = (value: unknown): value is number => Number.isSafeInteger(value) && Number(value) > 0;
const identifier = (value: unknown): value is string => typeof value === 'string' && /^[a-zA-Z0-9][a-zA-Z0-9:._-]{0,127}$/.test(value);
const digest = (value: unknown): value is string => typeof value === 'string' && /^sha256:[a-f0-9]{64}$/.test(value);
function closed(value: unknown, fields: string[]): asserts value is Record<string, unknown> {
  if (!isRecord(value) || value.schema_version !== 1 || Object.keys(value).length !== fields.length
      || fields.some((key) => !Object.hasOwn(value, key))) throw fail();
}
export function parseReleaseRecoveryCandidate(value: unknown, original?: string): ReleaseRecoveryCandidate {
  closed(value, ['schema_version', 'product_revision', 'owner', 'original_operation_id', 'original_command_digest',
    'original_release_ref', 'original_release_digest', 'config_revision', 'config_digest', 'evidence_digest']);
  if (!positive(value.product_revision) || !positive(value.config_revision) || !identifier(value.owner)
      || !identifier(value.original_operation_id) || (original !== undefined && original !== value.original_operation_id)
      || typeof value.original_release_ref !== 'string' || !/^[a-zA-Z0-9][a-zA-Z0-9._-]{0,95}$/.test(value.original_release_ref)
      || ![value.original_command_digest, value.original_release_digest, value.config_digest, value.evidence_digest].every(digest)) throw fail();
  return value as unknown as ReleaseRecoveryCandidate;
}
function parseRequest(value: unknown): ReleaseRecoveryRequest {
  closed(value, ['schema_version', 'operation_id', 'idempotency_key', 'expected_revision', 'owner', 'decision', 'candidate', 'requested_deadline']);
  const candidate = parseReleaseRecoveryCandidate(value.candidate);
  if (!identifier(value.operation_id) || !identifier(value.idempotency_key) || !positive(value.requested_deadline)
      || value.expected_revision !== candidate.product_revision || value.owner !== candidate.owner
      || value.decision !== 'abort_before_switch' || value.operation_id === candidate.original_operation_id) throw fail();
  return value as unknown as ReleaseRecoveryRequest;
}
export function parseReleaseRecovery(value: unknown, operation?: string): ReleaseRecoveryView {
  closed(value, ['schema_version', 'request', 'request_digest', 'revision', 'stage', 'receipt_digest']);
  const request = parseRequest(value.request);
  if (!digest(value.request_digest) || !positive(value.revision) || value.revision <= request.expected_revision
      || (operation !== undefined && operation !== request.operation_id)
      || !['intent', 'committed'].includes(String(value.stage))
      || (value.stage === 'intent' ? value.receipt_digest !== null : !digest(value.receipt_digest))) throw fail();
  return value as unknown as ReleaseRecoveryView;
}
export function parseReleaseRecoveries(value: unknown): ReleaseRecoveryList {
  closed(value, ['schema_version', 'product_revision', 'owner', 'operations']);
  if (!positive(value.product_revision) || !identifier(value.owner) || !Array.isArray(value.operations) || value.operations.length > 25) throw fail();
  let previous = Number.MAX_SAFE_INTEGER;
  const ids = new Set<string>();
  for (const item of value.operations) {
    const view = parseReleaseRecovery(item);
    if (view.request.owner !== value.owner || view.revision > value.product_revision || view.revision >= previous
        || ids.has(view.request.operation_id)) throw fail();
    previous = view.revision; ids.add(view.request.operation_id);
  }
  return value as unknown as ReleaseRecoveryList;
}
export function releaseRecoveryCommand(candidate: ReleaseRecoveryCandidate, operation: string, now: number): ReleaseRecoveryCommand {
  parseReleaseRecoveryCandidate(candidate);
  if (!identifier(operation) || operation === candidate.original_operation_id || !positive(now) || !positive(now + 120_000)) throw fail();
  return { schema_version: 1, operation_id: operation, idempotency_key: operation, expected_revision: candidate.product_revision,
    decision: 'abort_before_switch', candidate, requested_deadline: now + 120_000 };
}
export function releaseRecoveryVerification(view: ReleaseRecoveryView, revision: number, now: number): ReleaseRecoveryVerification {
  parseReleaseRecovery(view);
  if (view.stage !== 'intent' || !positive(revision) || revision < view.revision || !positive(now) || !positive(now + 120_000)) throw fail();
  return { schema_version: 1, operation_id: view.request.operation_id, request_digest: view.request_digest,
    expected_revision: revision, requested_deadline: now + 120_000 };
}
