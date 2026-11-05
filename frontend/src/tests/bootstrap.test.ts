import { test } from 'node:test';
import assert from 'node:assert/strict';
import { parseBootstrapCatalog, parseBootstrapCandidate, candidateLabel } from '../bootstrap.ts';
import { ApiError, WorkbenchClient } from '../api/client.ts';

const hash = `sha256:${'a'.repeat(64)}`;
const candidate = () => ({ schema_version: 2, candidate_ref: 'candidate:original', candidate_digest: hash,
  verification_epoch: 3, kind: 'existing_brain', status: 'available', owner_ref: 'owner:original',
  owner_source: 'verified_from_candidate', owner_binding_digest: hash, store_identity_digest: hash,
  schema_digest: hash, protection_state: 'protected', issue_ref: null });
const catalog = () => ({ schema_version: 2, operation_id: 'claim:original', selected: null,
  catalog: { schema_version: 2, verification_epoch: 3, catalog_digest: hash, candidates: [candidate()] } });

test('bootstrap catalog preserves actual owner and exact selected operation across refresh', () => {
  const view = parseBootstrapCatalog(catalog());
  assert.equal(view.operation, 'claim:original'); assert.equal(view.selected, null);
  assert.match(candidateLabel(view.candidates[0]), /owner:original/);
  assert.equal(parseBootstrapCatalog({ schema_version: 2, operation_id: 'claim:original', catalog: null, selected: candidate() }).selected, 'candidate:original');
  assert.throws(() => parseBootstrapCatalog({ ...catalog(), selected: candidate() }));
  for (const patch of [{ schema_version: 1 }, { candidates: [] }, { catalog_digest: 'forged' }, { verification_epoch: 4 }, { candidates: [candidate(), candidate()] }]) {
    const value = catalog(); Object.assign(value.catalog, patch); assert.throws(() => parseBootstrapCatalog(value));
  }
});

test('bootstrap candidate rejects unverifiable available identities and forged new-brain owners', () => {
  for (const patch of [{ schema_version: 1 }, { candidate_ref: '../path' }, { candidate_digest: 'forged' },
    { owner_ref: null }, { owner_source: 'not_established' }, { owner_binding_digest: null }, { protection_state: 'unknown' },
    { store_identity_digest: null }, { verification_epoch: 0 }, { issue_ref: 'issue:unverified' }]) {
    assert.throws(() => parseBootstrapCandidate({ ...candidate(), ...patch }));
  }
  const fresh = { ...candidate(), kind: 'new_brain_target', owner_ref: null, owner_source: 'generated_for_new_brain',
    owner_binding_digest: null, store_identity_digest: null, schema_digest: null };
  assert.equal(parseBootstrapCandidate(fresh).kind, 'new_brain_target');
  assert.throws(() => parseBootstrapCandidate({ ...fresh, owner_ref: 'owner:invented' }));
  assert.equal(parseBootstrapCandidate({ ...candidate(), status: 'unverifiable', owner_ref: null, protection_state: 'unknown', issue_ref: 'issue:unverified' }).status, 'unverifiable');
});

test('bootstrap selection and claim keep same-origin CSRF and original key without forwarding owner or paths', async () => {
  const calls: { path: string; request: RequestInit }[] = [];
  const api = new WorkbenchClient(async (path, request) => {
    calls.push({ path: String(path), request: request! });
    return Response.json(String(path).endsWith('/prove') ? { schema_version: 1, csrf: 'setup:csrf' }
      : String(path).endsWith('/select-v2') ? candidate()
      : { schema_version: 1, session: { owner: 'owner:original' }, csrf: 'session:csrf' });
  });
  await api.prove('local-fixture-proof', 'claim:original');
  await api.selectLegacy(parseBootstrapCandidate(candidate()), 'claim:original');
  await api.claimLegacy('candidate:original', 'fixture', 'fixture password only', 'claim:original');
  assert.deepEqual(calls.map(call => call.path), ['/api/v1/bootstrap/prove', '/api/v1/bootstrap/select-v2', '/api/v1/bootstrap/claim-v2']);
  for (const call of calls.slice(1)) {
    assert.equal(call.request.credentials, 'same-origin'); assert.equal(call.request.cache, 'no-store');
    assert.equal(new Headers(call.request.headers).get('Idempotency-Key'), 'claim:original');
    assert.equal(new Headers(call.request.headers).get('X-Forme-Csrf'), 'setup:csrf');
    assert.equal(call.request.signal?.aborted, false);
    const body = JSON.parse(String(call.request.body)); assert.equal(body.owner, undefined); assert.equal(body.path, undefined);
  }
  assert.deepEqual(JSON.parse(String(calls[1].request.body)), { schema_version: 2, candidate_ref: 'candidate:original', expected_candidate_digest: hash });
});

test('bootstrap unknown outcomes never retry, rediscover or fall back to blank data', async () => {
  const paths: string[] = [];
  const api = new WorkbenchClient(async (path) => { paths.push(String(path)); throw new TypeError('fixture disconnect'); });
  await assert.rejects(api.bootstrapCandidates(), (error: unknown) => error instanceof ApiError && error.code === 'ConnectionLost');
  await assert.rejects(api.claimLegacy('candidate:original', 'fixture', 'fixture password only', 'claim:original'));
  assert.deepEqual(paths, ['/api/v1/bootstrap/candidates', '/api/v1/bootstrap/claim-v2']);
});

test('bootstrap selection rejects rebound owner, epoch or data even when a server reuses a digest', async () => {
  for (const patch of [{ owner_ref: 'owner:foreign' }, { verification_epoch: 4 }, { candidate_ref: 'candidate:other' }, { kind: 'verified_backup' }]) {
    const api = new WorkbenchClient(async () => Response.json({ ...candidate(), ...patch }));
    await assert.rejects(api.selectLegacy(parseBootstrapCandidate(candidate()), 'claim:original'));
  }
});
