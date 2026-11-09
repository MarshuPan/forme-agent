import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { WorkbenchClient } from '../api/client.ts';
import { consentReady, desktopPendingRef, desktopApprovalOperation, parseDesktopCatalog, parseDesktopConsent } from '../desktop.ts';
import type { DesktopConsentRequest, DesktopConsentView } from '../desktop.ts';

const item = { approval_id: 'approval:fixture', plan_digest: `sha256:${'a'.repeat(64)}` };
const pending = `desktop-pending:${createHash('sha256').update(JSON.stringify(['forme.product.desktop.pending.v1', 'reference', item.approval_id, item.plan_digest])).digest('hex')}`;
const prepared: DesktopConsentView = {
  schema_version: 1, operation_id: `desktop-consent-request:${pending.split(':')[1]}`, revision: 2,
  pending_ref: pending, consent_ref: 'desktop-consent:fixture', status: 'prepared', expires_at: 8000, reason_ref: null,
};
const request: DesktopConsentRequest = {
  schema_version: 1, operation_id: prepared.operation_id, expected_revision: 2, pending_ref: pending,
  catalog_revision: 2, catalog_digest: `sha256:${'b'.repeat(64)}`, session_ref: 'desktop-session:fixture', expires_at: 8000,
};

test('desktop references and approval nonce bind the original ticket and plan across reloads', async () => {
  assert.equal(await desktopPendingRef(item), pending);
  const original = await desktopApprovalOperation(item);
  assert.equal(await desktopApprovalOperation(structuredClone(item)), original);
  assert.notEqual(await desktopApprovalOperation({ ...item, approval_id: 'approval:other' }), original);
  assert.notEqual(await desktopApprovalOperation({ ...item, plan_digest: `sha256:${'b'.repeat(64)}` }), original);
  await assert.rejects(desktopPendingRef({ ...item, plan_digest: 'not-a-plan' }));
});

test('only the original committed unexpired consent enables approval', () => {
  for (const status of ['prepared', 'revoked', 'expired', 'unknown'] as const) assert.equal(consentReady({ ...prepared, status }, 30), false);
  assert.equal(consentReady({ ...prepared, status: 'committed' }, 30), true);
  assert.equal(consentReady({ ...prepared, status: 'committed' }, 8000), false);
  for (const value of [{ ...prepared, pending_ref: 'pending:other' }, { ...prepared, status: 'success' }, { ...prepared, token: 'private' }, { ...prepared, schema_version: 2 }]) {
    assert.throws(() => parseDesktopConsent(value, pending));
  }
  assert.throws(() => parseDesktopConsent(prepared, pending, 'operation:other'));
});

test('lost consent response is recovered by GET with a fresh client and never a second POST', async () => {
  const calls: Array<{ path: string; request: RequestInit }> = [];
  let stored = prepared;
  const transport: typeof fetch = async (input, init) => {
    calls.push({ path: String(input), request: init! });
    if (init!.method === 'POST') {
      assert.equal(new Headers(init!.headers).get('Idempotency-Key'), prepared.operation_id);
      assert.deepEqual(JSON.parse(String(init!.body)), { schema_version: 1, request, password: 'fixture-password' });
      stored = { ...prepared, status: 'committed' };
      throw new TypeError('response lost');
    }
    return Response.json(stored);
  };
  const first = new WorkbenchClient(transport);
  assert.equal((await first.desktopConsent(pending)).status, 'prepared');
  await assert.rejects(first.consentDesktop(request, 'fixture-password'));
  const reloaded = new WorkbenchClient(transport);
  assert.equal((await reloaded.desktopConsent(await desktopPendingRef(item))).status, 'committed');
  assert.deepEqual(calls.map((call) => call.request.method), ['GET', 'POST', 'GET']);
  assert.ok(calls[2].path.endsWith(encodeURIComponent(pending)));
  assert.equal(calls[2].request.body, undefined);
  assert.equal(calls[2].request.cache, 'no-store');
});

test('lookup errors and foreign successful replies do not become prepared or committed', async () => {
  for (const response of [new Response('', { status: 503 }), Response.json({ ...prepared, pending_ref: 'pending:foreign' })]) {
    const api = new WorkbenchClient(async () => response);
    await assert.rejects(api.desktopConsent(pending));
  }
  const api = new WorkbenchClient(async () => Response.json({ ...prepared, status: 'committed', operation_id: 'operation:other' }));
  await assert.rejects(api.consentDesktop(request, 'fixture-password'));
});

test('catalog rejects stale, duplicate, oversized and unsupported choices', () => {
  const catalog = {
    schema_version: 1, installation_ref: 'installation:fixture', owner_ref: 'owner:fixture', authority_epoch: 1,
    catalog_revision: 2, catalog_digest: request.catalog_digest, observed_at: 10, expires_at: 30010,
    entries: [{ schema_version: 1, session_ref: 'desktop-session:fixture', platform: 'windows', label: '测试桌面', available: true, supported_capabilities: ['pointer_click'] }],
  };
  assert.equal(parseDesktopCatalog(catalog, 30).entries.length, 1);
  for (const value of [
    { ...catalog, expires_at: 30 }, { ...catalog, expires_at: 30011 },
    { ...catalog, entries: [catalog.entries[0], catalog.entries[0]] },
    { ...catalog, entries: [{ ...catalog.entries[0], label: '桌'.repeat(43) }] },
    { ...catalog, entries: [{ ...catalog.entries[0], supported_capabilities: ['shell'] }] },
    { ...catalog, os_session_ref: 'session:private' },
  ]) assert.throws(() => parseDesktopCatalog(value, 30));
});
