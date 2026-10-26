import { test } from 'node:test';
import assert from 'node:assert/strict';
import { WorkbenchClient } from '../api/client.ts';
import { emptyModel, patchFromDraft } from '../configuration.ts';
import { parseProductSettings, settingConfigurable, settingFeatures } from '../settings-schema.ts';
import type { ProductConfigView } from '../types.ts';

function view(): ProductConfigView {
  return { schema_version: 2, install_ref: 'install:settings', owner: 'owner:settings',
    active_revision: 1, acknowledged_revision: null, pending_restart: true, configured: true,
    models: [], brain: { schema_version: 1, default_workspace: 'workspace:settings',
      model_routing: { schema_version: 1, foreground: null, cognition: null } },
    brain_source: 'managed_configuration', capabilities: { schema_version: 1, skills: [] },
    capabilities_source: 'installation_template', issue_refs: [],
    network: { schema_version: 1, bind: '127.0.0.1:8765', allowed_hosts: [], tls: null, firewall: null },
    network_active: true, network_source: 'installation_template',
    proxy: { schema_version: 1, mode: 'direct', endpoint_configured: false }, proxy_source: 'installation_template',
    availability: { schema_version: 1, features: settingFeatures.map((feature) => {
      const configurable = ['models', 'skills', 'proxy', 'network'].includes(feature);
      return { schema_version: 1, feature, state: configurable ? 'configurable' : 'unavailable',
        reason: configurable ? null : 'managed_consumer_missing' };
    }) } };
}

test('settings read negotiates v2 explicitly and unavailable consumers stay distinct from configured values', async () => {
  const calls: string[] = [];
  const api = new WorkbenchClient(async (path, init) => {
    calls.push(String(path)); assert.equal(init?.method, 'GET');
    return Response.json(view());
  });
  const result = await api.productConfig();
  assert.deepEqual(calls, ['/api/v1/product/config?schema_version=2']);
  assert.equal(result.schema_version, 2);
  for (const feature of ['mcp', 'plugins', 'packages', 'channels', 'proxy']) {
    assert.equal(result.availability!.features.find((item) => item.feature === feature)?.state, feature === 'proxy' ? 'configurable' : 'unavailable');
  }
  assert.equal(Object.hasOwn(result, 'credential'), false);
});

test('unknown, incomplete, duplicate and contradictory settings capabilities are rejected', () => {
  assert.deepEqual(parseProductSettings(view()), view());
  for (const version of [0, 1, 3]) assert.throws(() => parseProductSettings({ ...view(), schema_version: version }));
  assert.throws(() => parseProductSettings({ ...view(), availability: undefined }));
  assert.throws(() => parseProductSettings({ ...view(), brain: { ...view().brain, schema_version: 2 } }));
  assert.throws(() => parseProductSettings({ ...view(), network: { ...view().network, schema_version: 2 } }));
  assert.throws(() => parseProductSettings({ ...view(), proxy: { ...view().proxy!, endpoint: 'http://secret.invalid:8080' } }));
  const source = view().availability!;
  assert.throws(() => parseProductSettings({ ...view(), availability: { ...source, features: source.features.slice(1) } }));
  assert.throws(() => parseProductSettings({ ...view(), availability: { ...source, features: source.features.map(() => source.features[0]) } }));
  assert.throws(() => parseProductSettings({ ...view(), availability: { ...source, features: source.features.map((item) => ({ ...item, state: 'configurable', reason: 'managed_consumer_missing' })) } }));
  assert.throws(() => parseProductSettings({ ...view(), network: { ...view().network, tls: {
    schema_version: 1, material_ref: 'managed:web', material_configured: true, certificate_path: 'secret', private_key_path: 'secret',
  } } }));
});

test('settings actions fail closed when a feature is unavailable or has no v2 availability proof', () => {
  const current = view();
  assert.equal(settingConfigurable(current, 'network'), true);
  assert.equal(settingConfigurable({ ...current, schema_version: 1, availability: undefined }, 'network'), false);
  assert.equal(settingConfigurable({ ...current, availability: undefined }, 'network'), false);
  const unavailable = { ...current, availability: { ...current.availability!, features: current.availability!.features.map((item) =>
    item.feature === 'network' ? { ...item, state: 'unavailable' as const, reason: 'network_paths_missing' as const } : item) } };
  assert.equal(settingConfigurable(unavailable, 'network'), false);
});

test('settings version support does not allow v2 on ordinary brain endpoints', async () => {
  const api = new WorkbenchClient(async () => Response.json({ schema_version: 2 }));
  await assert.rejects(api.runtime());
});

test('v2 writes retain nested v1 schemas and operation binding without sending readonly availability', async () => {
  const calls: { path: string; init: RequestInit }[] = [];
  const api = new WorkbenchClient(async (path, init) => {
    calls.push({ path: String(path), init: init! });
    return Response.json({ schema_version: 2, view: view(), applied: true, operation_id: 'settings:save' });
  });
  const saved = await api.saveProductConfig(0, patchFromDraft([emptyModel()]), 'settings:save');
  assert.equal(saved.view.schema_version, 2);
  assert.equal(calls.length, 1);
  const body = JSON.parse(String(calls[0].init.body));
  assert.equal(body.schema_version, 2);
  assert.equal(body.models[0].schema_version, 1);
  assert.equal(body.expected_revision, 0);
  assert.equal(new Headers(calls[0].init.headers).get('Idempotency-Key'), 'settings:save');
  assert.equal(Object.hasOwn(body, 'owner'), false);
  assert.equal(Object.hasOwn(body, 'availability'), false);
});

test('v2 network writes send only the managed material reference', async () => {
  const calls: RequestInit[] = [];
  const api = new WorkbenchClient(async (_path, init) => {
    calls.push(init!);
    return Response.json({ schema_version: 2, view: view(), applied: true, operation_id: 'settings:network' });
  });
  await api.saveProductConfig(0, patchFromDraft([emptyModel()]), 'settings:network', undefined, undefined, {
    ...view().network,
    bind: '192.168.1.20:8765',
    allowed_hosts: ['192.168.1.20:8765'],
    tls: { schema_version: 1, material_ref: 'managed:web', material_configured: true },
    firewall: { schema_version: 1, rule_ref: 'firewall:forme-web-lan' },
  });
  const body = JSON.parse(String(calls[0].body));
  assert.deepEqual(body.network.tls, { schema_version: 1, material_ref: 'managed:web' });
  assert.equal(Object.hasOwn(body.network.tls, 'material_configured'), false);
  assert.equal(Object.hasOwn(body.network.tls, 'certificate_path'), false);
  assert.equal(Object.hasOwn(body.network.tls, 'private_key_path'), false);
});

test('v2 proxy writes use a write-only endpoint and preserve public redaction', async () => {
  const calls: RequestInit[] = [];
  const api = new WorkbenchClient(async (_path, init) => {
    calls.push(init!);
    return init?.method === 'GET'
      ? Response.json(view())
      : Response.json({ schema_version: 2, view: { ...view(), proxy: { schema_version: 1, mode: 'http', endpoint_configured: true } }, applied: true, operation_id: 'settings:proxy' });
  });
  await api.saveProductConfig(0, patchFromDraft([emptyModel()]), 'settings:proxy', undefined, undefined, undefined, { schema_version: 1, mode: 'http', endpoint: 'http://127.0.0.1:8080' });
  const body = JSON.parse(String(calls[0].body));
  assert.deepEqual(body.proxy, { schema_version: 1, mode: 'http', endpoint: 'http://127.0.0.1:8080' });
  assert.equal(Object.hasOwn(body, 'availability'), false);
  assert.equal((await api.productConfig()).proxy?.endpoint_configured, false);
});

test('incompatible write responses never trigger a downgraded or fresh operation', async () => {
  let calls = 0;
  const api = new WorkbenchClient(async () => {
    calls++; return Response.json({ schema_version: 1, view: { ...view(), schema_version: 1 } });
  });
  await assert.rejects(api.saveProductConfig(0, [], 'settings:unknown'));
  assert.equal(calls, 1);
});
