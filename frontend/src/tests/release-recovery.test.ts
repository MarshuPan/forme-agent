import { test } from 'node:test';
import assert from 'node:assert/strict';
import { WorkbenchClient, ApiError } from '../api/client.ts';
import { maintenanceComplete, maintenanceTerminal } from '../maintenance.ts';
import { parseReleaseRecoveries, parseReleaseRecovery, parseReleaseRecoveryCandidate, releaseRecoveryCommand, releaseRecoveryVerification } from '../release-recovery.ts';
import type { ReleaseRecoveryCandidate, ReleaseRecoveryView } from '../release-recovery.ts';

const digest = (digit: string) => `sha256:${digit.repeat(64)}`;
const candidate: ReleaseRecoveryCandidate = { schema_version: 1, product_revision: 28, owner: 'owner:fixture',
  original_operation_id: 'upgrade:original', original_command_digest: digest('a'), original_release_ref: '0.1.0',
  original_release_digest: digest('b'), config_revision: 1, config_digest: digest('c'), evidence_digest: digest('d') };
const command = releaseRecoveryCommand(candidate, 'recover:original', 1_000);
const intent: ReleaseRecoveryView = { schema_version: 1, request: { ...command, owner: candidate.owner },
  request_digest: digest('e'), revision: 29, stage: 'intent', receipt_digest: null };
const committed: ReleaseRecoveryView = { ...intent, revision: 30, stage: 'committed', receipt_digest: digest('f') };

test('release recovery binds the exact candidate and distinguishes abort from maintenance success', () => {
  assert.equal(command.expected_revision, candidate.product_revision);
  assert.equal(command.idempotency_key, command.operation_id);
  assert.equal(command.requested_deadline, 121_000);
  assert.equal(Object.hasOwn(command, 'owner'), false);
  assert.deepEqual(parseReleaseRecovery(intent), intent);
  assert.deepEqual(parseReleaseRecovery(committed), committed);
  const op = { schema_version: 1 as const, operation_id: 'upgrade:original', stage: 'aborted', pending_stage: null };
  assert.equal(maintenanceComplete(op), false); assert.equal(maintenanceTerminal(op), true);
  assert.equal(maintenanceTerminal({ ...op, pending_stage: 'quiescing' }), false);
  assert.throws(() => releaseRecoveryVerification(committed, 30, 2_000));
  assert.throws(() => releaseRecoveryVerification(intent, 28, 2_000));
});

test('recovery parsers reject unsupported, extra, stale, unbound and cross-owner records', () => {
  for (const value of [{ ...candidate, schema_version: 2 }, { ...candidate, path: 'C:/foreign' },
    { ...candidate, original_operation_id: '../foreign' }, { ...candidate, evidence_digest: 'unbound' },
    { ...candidate, product_revision: NaN }]) assert.throws(() => parseReleaseRecoveryCandidate(value));
  assert.throws(() => parseReleaseRecoveryCandidate(candidate, 'upgrade:other'));
  for (const value of [{ ...intent, revision: 28 }, { ...intent, receipt_digest: digest('f') }, { ...committed, receipt_digest: null },
    { ...intent, request: { ...intent.request, owner: 'owner:foreign' } }, { ...intent, password: 'never-accept' },
    { ...intent, request: { ...intent.request, expected_revision: 27 } }]) assert.throws(() => parseReleaseRecovery(value));
  const list = { schema_version: 1, product_revision: 30, owner: candidate.owner, operations: [intent] };
  assert.deepEqual(parseReleaseRecoveries(list), list);
  assert.throws(() => parseReleaseRecoveries({ ...list, owner: 'owner:other' }));
  assert.throws(() => parseReleaseRecoveries({ ...list, operations: [intent, intent] }));
  assert.throws(() => parseReleaseRecoveries({ ...list, product_revision: 28 }));
  assert.throws(() => parseReleaseRecovery(intent, 'recover:other'));
});

test('lost recovery responses only query the original id; reload restores the durable intent without POST', async () => {
  const calls: { path: string; init: RequestInit }[] = [];
  const api = new WorkbenchClient(async (path, init) => {
    calls.push({ path: String(path), init: init! });
    if (init?.method === 'POST') throw new TypeError('fixture connection lost after intent');
    return Response.json(String(path).endsWith('/operations')
      ? { schema_version: 1, product_revision: 29, owner: candidate.owner, operations: [intent] } : intent);
  });
  await assert.rejects(api.recoverRelease(command, 'fixture-only-password'), (error: unknown) => error instanceof ApiError && error.status === 0);
  const body = JSON.parse(String(calls[0].init.body));
  assert.equal(body.password, 'fixture-only-password'); assert.equal(Object.hasOwn(body, 'owner'), false);
  assert.equal(new Headers(calls[0].init.headers).get('Idempotency-Key'), command.operation_id);
  assert.equal((await api.releaseRecoveryOperation(command.operation_id)).stage, 'intent');
  api.clear(); // A new session/page can recover solely from server facts.
  assert.deepEqual((await api.releaseRecoveries()).operations[0], intent);
  assert.equal(calls.length, 3);
  for (const call of calls.slice(1)) {
    assert.equal(call.init.method, 'GET'); assert.equal(call.init.body, undefined);
    assert.equal(new Headers(call.init.headers).has('Idempotency-Key'), false);
  }
  assert.equal(calls[1].path, '/api/v1/product/release-recovery/operations/recover%3Aoriginal');
});

test('only a distinct explicit verification posts the original request digest with current CAS', async () => {
  const calls: { path: string; init: RequestInit }[] = [];
  const api = new WorkbenchClient(async (path, init) => {
    calls.push({ path: String(path), init: init! }); return Response.json(committed);
  });
  const verification = releaseRecoveryVerification(intent, 29, 2_000);
  assert.equal((await api.verifyReleaseRecovery(verification, 'new-fixture-password')).stage, 'committed');
  assert.equal(calls.length, 1);
  assert.equal(calls[0].path, '/api/v1/product/release-recovery/verify');
  const body = JSON.parse(String(calls[0].init.body));
  assert.equal(body.operation_id, command.operation_id); assert.equal(body.request_digest, intent.request_digest);
  assert.equal(body.expected_revision, 29); assert.equal(body.requested_deadline, 122_000);
  assert.equal(Object.hasOwn(body, 'owner'), false);
  const bad = new WorkbenchClient(async () => Response.json({ ...committed, request_digest: digest('0') }));
  await assert.rejects(bad.verifyReleaseRecovery(verification, 'fixture-password'));
  const foreign = new WorkbenchClient(async () => Response.json({ ...committed, request: { ...committed.request, requested_deadline: 999_000 } }));
  await assert.rejects(foreign.recoverRelease(command, 'fixture-password'));
});
