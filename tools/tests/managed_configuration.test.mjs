// Real binaries and HTTP; service-manager behavior has its own contract tests.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { promises as fs } from 'node:fs';
import { createServer } from 'node:http';
import { once } from 'node:events';
import { createManagedProduct, readJson } from './fixtures/managed_product.mjs';
import { exerciseOfflineData } from './fixtures/offline_data_process.mjs';
import { exerciseRestoreWorker, setFixtureAdmission } from './fixtures/restore_worker_process.mjs';
import { exerciseWebRestoreCandidate } from './fixtures/restore_web_process.mjs';
import { exerciseWebArtifacts } from './fixtures/artifacts_web_process.mjs';
import { exerciseWebProjects } from './fixtures/projects_web_process.mjs';

const password = 'configuration process fixture 82';
const credential = 'fixture-model-key-not-a-real-secret';
const answer = 'Managed configuration process fixture answer.';
const skillBody = 'process-managed-skill-body-fixture';
const save = (baseUrl, revision = 0, operation = 'config:process-first') => ({ schema_version: 1,
  operation_id: operation, expected_revision: revision, models: [{ schema_version: 1, id: 'model:fixture',
    provider: 'openai-compatible', model: 'fixture-process-model', base_url: baseUrl, credential,
    context_window: 128000, tool_use: true, strength: 'standard', input_microunits_per_million: 0,
    output_microunits_per_million: 0, requests_per_minute: 600, tokens_per_minute: 1000000,
    timeout_ms: 10000, fallback_group: null, default: true, tool_schema_versions: [1] }] });

test('claim, Web config, restart recovery and real ProductionRuntime acknowledgement', {
  timeout: process.env.FORME_CONFIG_BROWSER_PREVIEW === '1' ? 600000 : 300000,
}, async (t) => {
  const product = await createManagedProduct();
  t.after(() => product.cleanup());
  const modelRequests = [];
  const model = createServer(async (request, response) => {
    try {
      let body = '';
      for await (const chunk of request) { body += chunk; assert.ok(body.length < 1024 * 1024); }
      assert.equal(request.headers.authorization, `Bearer ${credential}`);
      assert.equal(request.url, '/v1/chat/completions');
      modelRequests.push(JSON.parse(body));
      response.writeHead(200, { 'Content-Type': 'application/json' });
      response.end(JSON.stringify({ id: 'fixture:completion', object: 'chat.completion', created: 0,
        model: 'fixture-process-model', choices: [{ index: 0, message: { role: 'assistant', content: answer }, finish_reason: 'stop' }],
        usage: { prompt_tokens: 100, completion_tokens: 10, total_tokens: 110 } }));
    } catch { response.writeHead(400); response.end('{}'); }
  });
  model.listen(0, '127.0.0.1'); await once(model, 'listening');
  t.after(() => new Promise((resolve) => { model.close(resolve); model.closeAllConnections(); }));
  const baseUrl = `http://127.0.0.1:${model.address().port}/v1`;
  let host = await product.startHost();
  let web = await product.startWeb();
  const setup = 'claim:configuration-process';
  const status = await product.request('/api/v1/bootstrap/status');
  assert.equal(status.claimed, false);
  await product.request('/api/v1/product/config', { expected: 401 });
  await product.request('/api/v1/bootstrap/prove', { method: 'POST', key: setup,
    body: { schema_version: 1, proof: product.proof(setup) } });
  const candidate = await product.request('/api/v1/bootstrap/data');
  assert.equal(candidate.available, true);
  await product.request('/api/v1/bootstrap/data', { method: 'POST', key: setup,
    body: { schema_version: 1, candidate_ref: candidate.candidate_ref } });
  const session = await product.request('/api/v1/bootstrap/claim', { method: 'POST', key: setup,
    body: { schema_version: 1, operation_id: setup, candidate_ref: candidate.candidate_ref, login_name: 'fixture', password } });
  const empty = await product.request('/api/v1/product/config');
  assert.equal(empty.owner, session.session.owner);
  assert.equal(empty.configured, false);
  assert.equal(empty.brain_source, 'installation_template');
  assert.equal(empty.brain.default_workspace, product.template.workspace);
  const initial = save(baseUrl);
  initial.capabilities = { schema_version: 1, skills: [{ schema_version: 1, id: 'skill:process-fixture',
    summary: 'quantumorbit process fixture', scope: product.template.workspace, version: 1, body: skillBody,
    enabled: true, trust: 'owner_input' }] };
  const invalid = save('https://fixture.invalid/v1?key=forbidden', 0, 'config:invalid');
  await product.request('/api/v1/product/config', { method: 'POST', key: invalid.operation_id, body: invalid, expected: 422 });
  assert.equal((await product.request('/api/v1/product/config')).active_revision, 0);
  await product.request('/api/v1/product/config', { method: 'POST', key: initial.operation_id, body: initial,
    headers: { 'X-Forme-Csrf': 'wrong-fixture-csrf' }, expected: 403 });
  const saved = await product.request('/api/v1/product/config', { method: 'POST', key: initial.operation_id, body: initial });
  assert.equal(saved.applied, true);
  assert.equal(saved.view.active_revision, 1);
  assert.equal(saved.view.acknowledged_revision, null);
  assert.equal(saved.view.pending_restart, true);
  assert.deepEqual(saved.view.capabilities, initial.capabilities);
  assert.equal(JSON.stringify(saved).includes(credential), false);
  assert.equal(Object.hasOwn(saved.view.models[0], 'credential_path'), false);
  assert.equal(modelRequests.length, 0, 'configuration service must not call the provider');
  const validation = await product.request('/api/v1/product/config/validate', { method: 'POST',
    body: { schema_version: 1, revision: 1 } });
  assert.equal(validation.model_test_status, 'NotRun');
  assert.equal(validation.valid, true);
  const activation = { schema_version: 1, operation_id: 'activate:wrong-password', expected_revision: 1,
    action: 'start', requested_deadline: Date.now() + 120000, password: 'wrong fixture password' };
  const journalLength = (await fs.stat(product.host.journal_path)).size;
  await product.request('/api/v1/product/config/activate', { method: 'POST', key: activation.operation_id, body: activation, expected: 401 });
  assert.equal((await fs.stat(product.host.journal_path)).size, journalLength, 'wrong password must not reach host maintenance');
  const maintenance = await product.request('/api/v1/product/maintenance');
  assert.equal(maintenance.owner, session.session.owner);
  assert.ok(maintenance.product_revision > 0);
  assert.deepEqual(maintenance.operations, []);
  const ownerAction = { schema_version: 1, operation_id: 'maintenance:process',
    expected_revision: maintenance.product_revision, action: { kind: 'stop_core' },
    requested_deadline: Date.now() + 120000, password };
  await product.request('/api/v1/product/maintenance', { method: 'POST', key: ownerAction.operation_id,
    body: { ...ownerAction, password: 'incorrect fixture password' }, expected: 401 });
  await product.request('/api/v1/product/maintenance', { method: 'POST', key: ownerAction.operation_id,
    body: ownerAction, headers: { 'X-Forme-Csrf': 'wrong-fixture-csrf' }, expected: 403 });
  await product.request('/api/v1/product/maintenance', { method: 'POST', key: ownerAction.operation_id,
    body: { ...ownerAction, action: { kind: 'stop_core', command: 'forbidden' } }, expected: 400 });
  // A real successful password reaches host CAS, but a deliberately stale product
  // revision guarantees zero OS service calls, even if this PC has an installation.
  await product.request('/api/v1/product/maintenance', { method: 'POST', key: ownerAction.operation_id,
    body: { ...ownerAction, expected_revision: maintenance.product_revision + 100 }, expected: 409 });
  assert.equal((await fs.stat(product.host.journal_path)).size, journalLength);
  assert.equal(modelRequests.length, 0, 'maintenance must not use the model');
  await product.stop(web); await product.stop(host);
  host = await product.startHost(); web = await product.startWeb();
  const replay = await product.request('/api/v1/product/config', { method: 'POST', key: initial.operation_id, body: initial });
  assert.equal(replay.applied, false); assert.equal(replay.view.active_revision, 1);
  assert.equal((await product.request('/api/v1/auth/session')).session.owner, session.session.owner);
  const stale = save(baseUrl, 0, 'config:stale');
  await product.request('/api/v1/product/config', { method: 'POST', key: stale.operation_id, body: stale, expected: 409 });
  assert.equal((await product.request('/api/v1/product/config')).active_revision, 1);
  // Start the actual core as a supervised test process, not through the OS SCM.
  let core = await product.startCore();
  const acknowledged = await product.request('/api/v1/product/config');
  assert.equal(acknowledged.acknowledged_revision, 1); assert.equal(acknowledged.pending_restart, false);
  const runtime = await product.request('/api/v1/brain/runtime/manifest');
  assert.ok(runtime.model_profiles.includes('model:fixture'));
  const coreConfig = await readJson(product.template.core_config_path);
  assert.equal(coreConfig.brain.owner, session.session.owner);
  assert.deepEqual(coreConfig.brain.capabilities, initial.capabilities);
  assert.equal(JSON.stringify(coreConfig).includes(credential), false);
  const beforeArtifacts = modelRequests.length;
  const artifacts = await exerciseWebArtifacts(product, coreConfig);
  const projects = await exerciseWebProjects(product, runtime);
  assert.equal(modelRequests.length, beforeArtifacts, 'artifact operations must not call the model');
  const runKey = 'run:managed-configuration-fixture';
  const unregistered = {
    schema_version: 1, source: 'UserTurn', session: 'session:configuration-process', agent_profile: product.template.agent_profile,
    input: 'Verify the managed fixture configuration with quantumorbit context.', budget: null, idempotency_key: runKey };
  await product.request('/api/v1/brain/runs', { method: 'POST', key: runKey, expected: 400, body: unregistered });
  assert.equal(modelRequests.length, beforeArtifacts, 'unregistered sessions must not reach the model');
  const conversation = await product.request('/api/v1/sessions', { method: 'POST', key: 'session:configuration-process', body: {
    schema_version: 1, command: 'session:configuration-process', title: 'Managed process fixture', workspace: null } });
  const run = await product.request('/api/v1/brain/runs', { method: 'POST', key: runKey, expected: 202,
    body: { ...unregistered, session: conversation.session } });
  const terminal = new Set(['Complete', 'Failed', 'Aborted', 'Limited']);
  const deadline = Date.now() + 30000;
  let summary;
  do {
    summary = await product.request(`/api/v1/brain/runs/${encodeURIComponent(run)}`);
    if (terminal.has(summary.status)) break;
    await new Promise((resolve) => setTimeout(resolve, 100));
  } while (Date.now() < deadline);
  assert.equal(summary.status, 'Complete');
  const result = await product.request(`/api/v1/brain/runs/${encodeURIComponent(run)}/answer`);
  assert.equal(result.answer, answer);
  assert.ok(modelRequests.length > 0);
  assert.ok(modelRequests.every((request) => request.model === 'fixture-process-model'));
  assert.ok(JSON.stringify(modelRequests.at(-1)).includes(skillBody), 'trusted enabled skill body must reach the managed context');
  const stream = await product.request(`/api/v1/brain/runs/${encodeURIComponent(run)}/events?after=0`, { text: true });
  for (const kind of ['RunAccepted', 'SessionBound', 'ContextBuildFinished', 'ModelCallStarted', 'ModelCallFinished', 'RunComplete']) {
    assert.ok(stream.includes(kind), `runtime trace is missing ${kind}`);
  }
  const callsBeforeWebRestore = modelRequests.length;
  await exerciseWebRestoreCandidate(product, password, coreConfig);
  assert.equal(modelRequests.length, callsBeforeWebRestore, 'preparing a restore must not call the model');
  if (process.env.FORME_CONFIG_BROWSER_PREVIEW === '1') {
    // Explicit local inspection only; CI never pauses. Fixture login is above.
    console.log(`FORME_FIXTURE_BROWSER_URL=${product.origin}`);
    await new Promise((resolve) => { process.stdin.once('data', resolve); process.stdin.resume(); });
    process.stdin.pause();
  }
  const callsBeforeMaintenance = modelRequests.length;
  const offline = await exerciseOfflineData(product, coreConfig);
  assert.equal(modelRequests.length, callsBeforeMaintenance, 'offline maintenance must not construct or call a model');
  // New config is saved while old core continues using its immutable key file.
  const second = save(baseUrl, 1, 'config:process-second');
  second.models[0].credential = 'rotated-fixture-key-not-a-real-secret';
  second.models.push({ ...second.models[0], id: 'model:foreground', model: 'fixture-foreground-model',
    credential, default: false });
  second.brain = { schema_version: 1, default_workspace: 'workspace:configured',
    model_routing: { schema_version: 1, foreground: 'model:foreground', cognition: 'model:foreground' } };
  const badRoute = { ...second, operation_id: 'config:absent-role', brain: { ...second.brain,
    model_routing: { ...second.brain.model_routing, cognition: 'model:absent' } } };
  await product.request('/api/v1/product/config', { method: 'POST', key: badRoute.operation_id, body: badRoute, expected: 422 });
  assert.equal((await product.request('/api/v1/product/config')).active_revision, 1);
  await product.request('/api/v1/product/config', { method: 'POST', key: second.operation_id, body: second });
  const pending = await product.request('/api/v1/product/config');
  assert.equal(pending.active_revision, 2); assert.equal(pending.acknowledged_revision, 1); assert.equal(pending.pending_restart, true);
  assert.deepEqual(pending.brain, second.brain);
  assert.equal(pending.brain_source, 'managed_configuration');
  assert.deepEqual(pending.capabilities, initial.capabilities);
  assert.equal(await fs.readFile(coreConfig.brain.models[0].credential_path, 'utf8'), credential);
  await product.stop(core);
  // The fixture deliberately kills the process; respect its original writer lease.
  await new Promise((resolve) => setTimeout(resolve, product.template.authority_lease + 200));
  core = await product.startCore();
  assert.equal((await product.request('/api/v1/product/config')).acknowledged_revision, 2);
  await artifacts.verify();
  await projects.verify();
  // Acknowledgement means loaded, not that the rotated provider credential works.
  assert.equal((await readJson(product.template.configuration_ack_path)).revision, 2);
  const newManifest = await product.request('/api/v1/brain/runtime/manifest');
  assert.equal(newManifest.workspace, 'workspace:configured');
  const configuredSession = await product.request('/api/v1/sessions', { method: 'POST', key: 'session:configured',
    body: { schema_version: 1, command: 'session:configured', title: 'Configured workspace', workspace: null } });
  const configuredKey = 'run:managed-preferences';
  const configuredRun = await product.request('/api/v1/brain/runs', { method: 'POST', key: configuredKey, expected: 202,
    body: { ...unregistered, session: configuredSession.session, idempotency_key: configuredKey } });
  const configuredDeadline = Date.now() + 30000;
  let configuredSummary;
  do {
    configuredSummary = await product.request(`/api/v1/brain/runs/${encodeURIComponent(configuredRun)}`);
    if (terminal.has(configuredSummary.status)) break;
    await new Promise((resolve) => setTimeout(resolve, 100));
  } while (Date.now() < configuredDeadline);
  assert.equal(configuredSummary.status, 'Complete');
  assert.equal(configuredSummary.workspace, 'workspace:configured');
  assert.equal(modelRequests.at(-1).model, 'fixture-foreground-model');
  assert.equal(JSON.stringify(modelRequests.at(-1)).includes(skillBody), false,
    'a skill scoped to the original workspace must not cross into the configured workspace');
  const configuredEvents = await product.request(`/api/v1/brain/runs/${encodeURIComponent(configuredRun)}/events?after=0`, { text: true });
  assert.ok(configuredEvents.includes('workspace:configured'));
  assert.ok(configuredEvents.includes('openai-compatible:fixture-foreground-model'));
  const callsAfterPreferences = modelRequests.length;
  await product.stop(core);
  // The explicit offline route reaches a fixed installed hostctl process,
  // not the stopped core or the fixture provider. No restore capability.
  const configBeforeVerification = await fs.readFile(product.template.core_config_path);
  const verified = await product.request('/api/v1/product/data/verify', { method: 'POST',
    body: { schema_version: 1, source: 'offline-process-source.sqlite', config_revision: 2 } });
  assert.equal(verified.config_revision, 2);
  assert.equal(verified.verification.object, 'offline-process-source.sqlite');
  assert.equal(verified.verification.verified, true);
  assert.equal(verified.verification.restoration, undefined);
  assert.equal(JSON.stringify(verified).includes(product.root), false);
  assert.deepEqual(await fs.readFile(product.template.core_config_path), configBeforeVerification);
  assert.equal(modelRequests.length, callsAfterPreferences);
  // A reachable configuration bridge rejects a stale revision as 502;
  // 503 is reserved here for an absent configuration bridge/core capability.
  await product.request('/api/v1/product/data/verify', { method: 'POST', expected: 502,
    body: { schema_version: 1, source: 'offline-process-source.sqlite', config_revision: 1 } });
  await product.request('/api/v1/product/data/verify', { method: 'POST', expected: 400,
    body: { schema_version: 1, source: '../offline-process-source.sqlite', config_revision: 2 } });
  // This is an explicit temporary test-process selection, not a production
  // active-pointer/Host restore transaction or a native service restart.
  await new Promise((resolve) => setTimeout(resolve, product.template.authority_lease + 200));
  await exerciseRestoreWorker(product, offline, password);
  await setFixtureAdmission(product, offline, false);
  const restoredCore = await product.start('forme-gatewayd', offline.restoredConfigPath,
    `FORME_GATEWAY_URL=http://${product.coreAddress}`);
  await product.request(`/api/v1/brain/runs/${encodeURIComponent(run)}/answer`, { expected: 503 });
  await setFixtureAdmission(product, offline, true);
  const restoredAnswer = await product.request(`/api/v1/brain/runs/${encodeURIComponent(run)}/answer`);
  assert.equal(restoredAnswer.answer, answer);
  assert.equal(modelRequests.length, callsAfterPreferences, 'restored read-only query must not call a model');
  offline.run('inspect-data', offline.restoredConfigPath, [], false);
  await product.stop(restoredCore); await product.stop(web); await product.stop(host);
});
