import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { WorkbenchClient, ApiError } from '../api/client.ts';
import { legacyEvidenceKinds, legacyRecoveryCommand, legacyRecoveryVerification, legacyReviewDigest,
  parseLegacyReview, parseLegacyRecovery, parseLegacyRecoveries, verifyLegacyRecoveryDigest } from '../legacy-release-recovery.ts';
import type { LegacyRecoveryView, LegacyReleaseReview } from '../legacy-release-recovery.ts';

const hash = (label: string) => `sha256:${createHash('sha256').update(label).digest('hex')}`;
// Same declared fixture and golden values as the Rust protocol contract test.
const bindingDigest = 'sha256:5330285943e7ea4964a0848cdf8a1dd805efa4e0767c92db0422abd0259af665';
const reportDigest = 'sha256:9be2f960600222cad01a2387f83bc643d12f31e1ae184526b90ed5d514f3a4bc';
const requestDigest = 'sha256:caf04d5763c2ccc8aad79a17ed39ca3bedf2cb5754937e4dbb495da32a456b1d';
const review: LegacyReleaseReview = { schema_version: 1, binding: {
  schema_version: 1, review_id: 'review:contract', install_ref: 'install:contract', owner: 'owner:contract',
  original_operation_id: 'upgrade:contract', original_command_digest: hash('command'), original_release_ref: '1.0.0',
  original_release_digest: hash('release'), config_revision: 2, config_digest: hash('config'), journal_seq: 10,
  journal_head: hash('head'), expires_at: 100_000,
}, evidence: legacyEvidenceKinds.map((kind) => ({ schema_version: 1, kind, artifact_digest: hash('artifact'),
  binding_digest: bindingDigest, observation_digest: hash('observation') })), status: 'verified', issues: [] };
const command = { schema_version: 1 as const, operation_id: 'decision:contract', idempotency_key: 'key:decision',
  expected_revision: 10, review_id: review.binding.review_id, review_digest: reportDigest, requested_deadline: 90_000 };
const intent: LegacyRecoveryView = { schema_version: 1, request: { ...command, owner: review.binding.owner },
  request_digest: requestDigest, revision: 11, stage: 'intent', review_digest: reportDigest, receipt_digest: null };
const committed: LegacyRecoveryView = { ...intent, revision: 12, stage: 'committed', receipt_digest: hash('receipt') };

function reorder(value: unknown): unknown {
  if (Array.isArray(value)) return value.map(reorder);
  if (value && typeof value === 'object') return Object.fromEntries(Object.entries(value).reverse().map(([key, item]) => [key, reorder(item)]));
  return value;
}

test('legacy browser digests match the Rust wire golden independent of JSON property ordering', async () => {
  assert.equal(await legacyReviewDigest(review), reportDigest);
  assert.equal(await legacyReviewDigest(parseLegacyReview(reorder(review))), reportDigest);
  assert.deepEqual(await verifyLegacyRecoveryDigest(intent), intent);
  assert.deepEqual(await verifyLegacyRecoveryDigest(parseLegacyRecovery(reorder(intent))), intent);
  const wrongBinding = structuredClone(review); wrongBinding.evidence[0].binding_digest = hash('foreign-binding');
  await assert.rejects(legacyReviewDigest(wrongBinding));
  await assert.rejects(verifyLegacyRecoveryDigest({ ...intent, request: { ...intent.request, requested_deadline: 90_001 } }));
});

test('legacy review rejects missing reordered unknown evidence and incomplete status claims', () => {
  for (const changed of [{ ...review, path: '/injected' }, { ...review, schema_version: 2 },
    { ...review, evidence: review.evidence.slice(1) }, { ...review, evidence: review.evidence.toReversed() },
    { ...review, evidence: [{ ...review.evidence[0], kind: 'caller_assertion' }] },
    { ...review, issues: ['issue:not-verified'] }, { ...review, status: 'unverifiable' },
    { ...review, binding: { ...review.binding, journal_seq: Number.MAX_SAFE_INTEGER + 1 } }]) assert.throws(() => parseLegacyReview(changed));
  assert.throws(() => parseLegacyReview(review, 'upgrade:other'));
  const unavailable = { ...review, status: 'unverifiable', evidence: [], issues: ['issue:missing-archive'] };
  assert.equal(parseLegacyReview(unavailable).status, 'unverifiable');
  assert.throws(() => parseLegacyReview({ ...unavailable, issues: ['issue:a', 'issue:a'] }));
});

test('legacy decisions preserve owner watermarks stage receipt boundaries and report lifetime', () => {
  assert.deepEqual(parseLegacyRecovery(committed), committed);
  for (const changed of [{ ...intent, revision: 10 }, { ...intent, receipt_digest: hash('receipt') },
    { ...committed, receipt_digest: null }, { ...intent, review_digest: hash('swapped') },
    { ...intent, evidence_path: '/injected' }]) assert.throws(() => parseLegacyRecovery(changed));
  const list = { schema_version: 1, product_revision: 11, owner: review.binding.owner, operations: [intent] };
  assert.deepEqual(parseLegacyRecoveries(list), list);
  for (const changed of [{ ...list, owner: 'owner:other' }, { ...list, product_revision: 10 },
    { ...list, operations: [intent, intent] }, { ...list, operations: [intent, committed] }]) assert.throws(() => parseLegacyRecoveries(changed));
  const fresh = legacyRecoveryCommand(review, reportDigest, 'decision:new', 1_000);
  assert.equal(fresh.expected_revision, 10); assert.equal(fresh.requested_deadline, 100_000);
  assert.equal(Object.hasOwn(fresh, 'owner'), false);
  assert.throws(() => legacyRecoveryCommand(review, reportDigest, 'upgrade:contract', 1_000));
  assert.throws(() => legacyRecoveryCommand(review, reportDigest, 'decision:new', 100_000));
  assert.throws(() => legacyRecoveryCommand({ ...review, status: 'rejected', issues: ['issue:rejected'] }, reportDigest, 'decision:new', 1_000));
  // Verification may outlive the initial request, but cannot outlive its report.
  assert.equal(legacyRecoveryVerification(intent, 11, 91_000, review).requested_deadline, 100_000);
  assert.throws(() => legacyRecoveryVerification(intent, 12, 91_000, review));
  assert.throws(() => legacyRecoveryVerification(intent, 11, 100_000, review));
  assert.throws(() => legacyRecoveryVerification(committed, 12, 91_000, review));
  assert.throws(() => legacyRecoveryVerification(intent, 11, 91_000, { ...review, binding: { ...review.binding, owner: 'owner:foreign' } }));
});

test('legacy lost response and reload only read the original operation and bound report', async () => {
  const calls: { path: string; init: RequestInit }[] = [];
  const api = new WorkbenchClient(async (path, init) => {
    calls.push({ path: String(path), init: init! });
    if (init?.method === 'POST') throw new TypeError('fixture lost response after intent');
    if (String(path).includes('/review/')) return Response.json(review);
    return Response.json(String(path).endsWith('/operations')
      ? { schema_version: 1, product_revision: 11, owner: review.binding.owner, operations: [intent] } : intent);
  });
  await assert.rejects(api.recoverLegacyRelease(command, 'legacy-fixture-password'), (error: unknown) => error instanceof ApiError && error.status === 0);
  const body = JSON.parse(String(calls[0].init.body));
  assert.equal(body.password, 'legacy-fixture-password'); assert.equal(Object.hasOwn(body, 'owner'), false);
  assert.equal(new Headers(calls[0].init.headers).get('Idempotency-Key'), command.operation_id);
  assert.deepEqual(await api.legacyReleaseRecoveryOperation(command.operation_id), intent);
  api.clear();
  assert.deepEqual((await api.legacyReleaseRecoveries()).operations[0], intent);
  assert.equal(await legacyReviewDigest(await api.legacyReleaseReview('upgrade:contract')), reportDigest);
  assert.equal(calls.length, 4);
  for (const call of calls.slice(1)) { assert.equal(call.init.method, 'GET'); assert.equal(call.init.body, undefined); }
});

test('legacy verification is a separate explicit post and rejects swapped request receipts', async () => {
  const calls: { path: string; init: RequestInit }[] = [];
  const api = new WorkbenchClient(async (path, init) => { calls.push({ path: String(path), init: init! }); return Response.json(committed); });
  const verify = legacyRecoveryVerification(intent, 11, 91_000, review);
  assert.deepEqual(await api.verifyLegacyReleaseRecovery(verify, 'fresh-fixture-password'), committed);
  assert.equal(calls.length, 1); assert.equal(calls[0].path, '/api/v1/product/legacy-release-recovery/verify');
  const body = JSON.parse(String(calls[0].init.body));
  assert.equal(body.request_digest, requestDigest); assert.equal(body.expected_revision, 11);
  assert.equal(body.requested_deadline, 100_000); assert.equal(Object.hasOwn(body, 'owner'), false);
  const changed = new WorkbenchClient(async () => Response.json({ ...committed, request: { ...committed.request, requested_deadline: 90_001 } }));
  await assert.rejects(changed.recoverLegacyRelease(command, 'fixture-password'));
  await assert.rejects(changed.verifyLegacyReleaseRecovery(verify, 'fixture-password'));
  const extra = new WorkbenchClient(async () => Response.json({ ...committed, public_key: 'injected' }));
  await assert.rejects(extra.verifyLegacyReleaseRecovery(verify, 'fixture-password'));
});
