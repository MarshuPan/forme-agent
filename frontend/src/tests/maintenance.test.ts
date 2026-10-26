import { test } from 'node:test';
import assert from 'node:assert/strict';
import { ApiError, WorkbenchClient } from '../api/client.ts';
import { configureNetworkCommand, maintenanceComplete, releaseMaintenanceCommand, restoreCommand } from '../maintenance.ts';
import type { MaintenanceCommand, MaintenanceSnapshot } from '../maintenance.ts';
import type { RestoreCandidate } from '../data.ts';
import type { ProductConfigValidation, ProductConfigView } from '../types.ts';

test('owner maintenance lost response only probes the original operation, without repeating credentials', async () => {
  const calls: { path: string; init: RequestInit }[] = [];
  const command: MaintenanceCommand = { schema_version: 1, operation_id: 'maintain:fixture', expected_revision: 13,
    action: { kind: 'uninstall_plan' }, requested_deadline: 120_000 };
  const api = new WorkbenchClient(async (path, init) => {
    calls.push({ path: String(path), init: init! });
    if (init?.method === 'POST') throw new TypeError('lost response fixture');
    return Response.json({ schema_version: 1, operation_id: command.operation_id, stage: 'committed', pending_stage: null });
  });
  await assert.rejects(api.maintain(command, 'fixture-password'), (error: unknown) => error instanceof ApiError && error.status === 0);
  assert.equal(calls.length, 1);
  assert.equal(new Headers(calls[0].init.headers).get('Idempotency-Key'), command.operation_id);
  const body = JSON.parse(String(calls[0].init.body));
  assert.equal(body.expected_revision, 13);
  assert.equal(Object.hasOwn(body, 'owner'), false);
  assert.deepEqual(body.action, { kind: 'uninstall_plan' });
  const result = await api.maintenanceOperation(command.operation_id);
  assert.equal(calls.length, 2);
  assert.equal(calls[1].path, '/api/v1/product/maintenance/operations/maintain%3Afixture');
  assert.equal(calls[1].init.method, 'GET');
  assert.equal(calls[1].init.body, undefined);
  assert.equal(maintenanceComplete(result), true);
  assert.equal(maintenanceComplete({ ...result, pending_stage: 'checking' }), false);
  assert.equal(maintenanceComplete({ ...result, stage: 'recovery_required' }), false);
});

const digest = (digit: string) => `sha256:${digit.repeat(64)}`;
const candidate: RestoreCandidate = { schema_version: 1, candidate_ref: 'restored.sqlite',
  candidate_digest: digest('a'), owner_digest: digest('b'), store_identity: digest('c'), execution_fence_digest: digest('d') };
const config: ProductConfigView = { schema_version: 1, configured: true, install_ref: 'installation:fixture',
  active_revision: 7, acknowledged_revision: 7, owner: 'owner:fixture', pending_restart: false, models: [], issue_refs: [],
  brain_source: 'installation_template', brain: { schema_version: 1, default_workspace: 'workspace:default',
    model_routing: { schema_version: 1, foreground: null, cognition: null } },
  network: { schema_version: 1, bind: '127.0.0.1:8765', allowed_hosts: [], tls: null, firewall: null },
  network_active: true, network_source: 'installation_template',
  proxy: { schema_version: 1, mode: 'direct', endpoint_configured: false }, proxy_source: 'installation_template' };
const snapshot: MaintenanceSnapshot = { schema_version: 1, product_revision: 31, owner: 'owner:fixture',
  host: { schema_version: 1, product_state: 'ready', core_health: 'ready', active_release: '0.0.1', allowed_operations: ['commit_restore'] }, operations: [] };

test('release changes bind exact version, digest, backup and operation and never resend on lost response', async () => {
  for (const kind of ['upgrade', 'rollback_compatible_release'] as const) {
    const view = { ...snapshot, host: { ...snapshot.host, allowed_operations: [kind] } };
    const command = releaseMaintenanceCommand(kind, '0.0.2', digest('a'), 'backup:original', view, 'release:original', 1_000);
    assert.deepEqual(command, { schema_version: 1, operation_id: 'release:original', expected_revision: 31,
      requested_deadline: 121_000, action: { kind, release_id: '0.0.2', release_digest: digest('a'), backup_ref: 'backup:original' } });
    const calls: { path: string; init: RequestInit }[] = [];
    const api = new WorkbenchClient(async (path, init) => {
      calls.push({ path: String(path), init: init! });
      if (init?.method === 'POST') throw new TypeError('release response lost');
      return Response.json({ schema_version: 1, operation_id: command.operation_id, stage: 'recovery_required', pending_stage: 'checking' });
    });
    await assert.rejects(api.maintain(command, 'fixture-release-password'));
    const receipt = await api.maintenanceOperation(command.operation_id);
    assert.equal(maintenanceComplete(receipt), false);
    assert.equal(calls.length, 2);
    assert.equal(calls[1].init.method, 'GET');
    assert.equal(calls[1].init.body, undefined);
    assert.equal(calls[1].path, '/api/v1/product/maintenance/operations/release%3Aoriginal');
    const body = JSON.parse(String(calls[0].init.body));
    assert.deepEqual(body.action, command.action);
    for (const field of ['owner', 'program', 'path', 'principal', 'verified']) assert.equal(Object.hasOwn(body, field), false);
    assert.throws(() => releaseMaintenanceCommand(kind, '0.0.2', digest('a'), 'backup:original', snapshot, 'release:original', 1_000));
    assert.throws(() => releaseMaintenanceCommand(kind, '../foreign', digest('a'), 'backup:original', view, 'release:original', 1_000));
    assert.throws(() => releaseMaintenanceCommand(kind, '0.0.2', 'unbound', 'backup:original', view, 'release:original', 1_000));
    assert.throws(() => releaseMaintenanceCommand(kind, '0.0.2', digest('a'), 'C:/backup', view, 'release:original', 1_000));
  }
});

const networkConfig = { ...config, network_active: false };
const networkSnapshot = { ...snapshot, host: { ...snapshot.host, allowed_operations: ['configure_lan'] } };
const networkValidation: ProductConfigValidation = { schema_version: 1, revision: 7, valid: true,
  restart_required: true, model_test_status: 'NotRun', issue_refs: [], network_validation_digest: digest('e') };

test('settings v2 remains compatible with the independently versioned maintenance contracts', () => {
  const current: ProductConfigView = { ...config, schema_version: 2 };
  assert.deepEqual(restoreCommand(candidate, current, snapshot, 'restore:v2', 1_000),
    restoreCommand(candidate, config, snapshot, 'restore:v2', 1_000));
  assert.deepEqual(configureNetworkCommand({ ...current, network_active: false }, networkValidation, networkSnapshot, 'network:v2', 1_000),
    configureNetworkCommand(networkConfig, networkValidation, networkSnapshot, 'network:v2', 1_000));
  const unknown = { ...current, schema_version: 3 } as unknown as ProductConfigView;
  assert.throws(() => restoreCommand(candidate, unknown, snapshot, 'restore:v2', 1_000));
  assert.throws(() => configureNetworkCommand({ ...unknown, network_active: false }, networkValidation, networkSnapshot, 'network:v2', 1_000));
});

test('network maintenance binds checked certificate digest and both revisions without client authority', async () => {
  const command = configureNetworkCommand(networkConfig, networkValidation, networkSnapshot, 'network:fixture', 1_000);
  assert.deepEqual(command, { schema_version: 1, operation_id: 'network:fixture', expected_revision: 31,
    requested_deadline: 121_000, action: { kind: 'configure_lan', config_revision: 7, validation_digest: digest('e') } });
  const calls: { path: string; init: RequestInit }[] = [];
  const api = new WorkbenchClient(async (path, init) => {
    calls.push({ path: String(path), init: init! });
    if (init?.method === 'POST') throw new TypeError('network switch disconnected');
    return Response.json({ schema_version: 1, operation_id: command.operation_id, stage: 'recovery_required', pending_stage: 'checking' });
  });
  await assert.rejects(api.maintain(command, 'fixture-network-password'));
  assert.equal(maintenanceComplete(await api.maintenanceOperation(command.operation_id)), false);
  assert.equal(calls.length, 2);
  assert.equal(calls[1].init.method, 'GET');
  assert.equal(calls[1].init.body, undefined);
  const body = JSON.parse(String(calls[0].init.body));
  for (const field of ['owner', 'network', 'certificate_path', 'private_key_path', 'principal', 'bind']) {
    assert.equal(Object.hasOwn(body, field), false);
  }
});

test('network apply refuses unsupported adapter, stale validation, owner or published configuration', () => {
  for (const changed of [ { ...networkValidation, revision: 8 }, { ...networkValidation, valid: false },
    { ...networkValidation, network_validation_digest: 'unbound' } ]) {
    assert.throws(() => configureNetworkCommand(networkConfig, changed, networkSnapshot, 'network:fixture', 1_000));
  }
  for (const changed of [config, { ...networkConfig, owner: 'owner:foreign' }, { ...networkConfig, configured: false }]) {
    assert.throws(() => configureNetworkCommand(changed, networkValidation, networkSnapshot, 'network:fixture', 1_000));
  }
  assert.throws(() => configureNetworkCommand(networkConfig, networkValidation, snapshot, 'network:fixture', 1_000));
  assert.throws(() => configureNetworkCommand(networkConfig, networkValidation, networkSnapshot, '../op', 1_000));
});

test('restore activation binds final manifest, owner and both revisions without storage paths or client authority', () => {
  const command = restoreCommand(candidate, config, snapshot, 'restore:fixture', 1_000);
  assert.deepEqual(command, { schema_version: 1, operation_id: 'restore:fixture', expected_revision: 31,
    requested_deadline: 121_000, action: { kind: 'commit_restore', candidate_ref: 'restored.sqlite',
      candidate_digest: digest('a'), owner_digest: digest('b'), config_revision: 7 } });
  const encoded = JSON.stringify(command);
  for (const field of ['store_identity', 'execution_fence_digest', 'path', 'verified', 'principal', 'owner:fixture']) {
    assert.equal(encoded.includes(field), false);
  }
});

test('restore activation refuses unavailable authority, stale owner and malformed candidate before submission', () => {
  for (const changed of [ { ...candidate, candidate_ref: '../foreign.sqlite' },
    { ...candidate, candidate_digest: 'database-digest' }, { ...candidate, execution_fence_digest: '' } ]) {
    assert.throws(() => restoreCommand(changed, config, snapshot, 'restore:fixture', 1_000));
  }
  for (const changed of [ { ...config, configured: false }, { ...config, owner: 'owner:foreign' },
    { ...config, active_revision: 0 }, { ...config, active_revision: NaN } ]) {
    assert.throws(() => restoreCommand(candidate, changed, snapshot, 'restore:fixture', 1_000));
  }
  assert.throws(() => restoreCommand(candidate, config, { ...snapshot, host: { ...snapshot.host, allowed_operations: [] } }, 'restore:fixture', 1_000));
  assert.throws(() => restoreCommand(candidate, config, { ...snapshot, product_revision: 1.5 }, 'restore:fixture', 1_000));
  assert.throws(() => restoreCommand(candidate, config, snapshot, '../operation', 1_000));
});

test('lost restore response is only inspected with the original operation even after a new login', async () => {
  const requests: { path: string; init: RequestInit }[] = [];
  const api = new WorkbenchClient(async (path, init) => {
    requests.push({ path: String(path), init: init! });
    if (init?.method === 'POST') throw new TypeError('session fenced after restore dispatch');
    return Response.json({ schema_version: 1, operation_id: 'restore:fixture', stage: 'recovery_required', pending_stage: 'checking' });
  });
  const command = restoreCommand(candidate, config, snapshot, 'restore:fixture', 1_000);
  await assert.rejects(api.maintain(command, 'fixture-restore-password'));
  const operation = await api.maintenanceOperation(command.operation_id);
  assert.equal(maintenanceComplete(operation), false);
  assert.equal(requests.length, 2);
  assert.equal(requests[1].path, '/api/v1/product/maintenance/operations/restore%3Afixture');
  assert.equal(requests[1].init.method, 'GET');
  assert.equal(requests[1].init.body, undefined);
  assert.equal(new Headers(requests[1].init.headers).has('Idempotency-Key'), false);
});
