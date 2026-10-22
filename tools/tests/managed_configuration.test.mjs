// Real binaries and HTTP; service-manager behavior has its own contract tests.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { promises as fs } from 'node:fs';
import { createServer } from 'node:http';
import { once } from 'node:events';
import { createManagedProduct, readJson } from './fixtures/managed_product.mjs';

const password = 'configuration process fixture 82';
const credential = 'fixture-model-key-not-a-real-secret';
const answer = 'Managed configuration process fixture answer.';
const save = (baseUrl, revision = 0, operation = 'config:process-first') => ({ schema_version: 1,
  operation_id: operation, expected_revision: revision, models: [{ schema_version: 1, id: 'model:fixture',
    provider: 'openai-compatible', model: 'fixture-process-model', base_url: baseUrl, credential,
    context_window: 128000, tool_use: true, strength: 'standard', input_microunits_per_million: 0,
    output_microunits_per_million: 0, requests_per_minute: 600, tokens_per_minute: 1000000,
    timeout_ms: 10000, fallback_group: null, default: true, tool_schema_versions: [1] }] });

test('claim, Web config, restart recovery and real ProductionRuntime acknowledgement', {
  timeout: process.env.FORME_CONFIG_BROWSER_PREVIEW === '1' ? 600000 : 180000,
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
  const initial = save(baseUrl);
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
  assert.equal(JSON.stringify(coreConfig).includes(credential), false);
  const runKey = 'run:managed-configuration-fixture';
  const run = await product.request('/api/v1/brain/runs', { method: 'POST', key: runKey, expected: 202, body: {
    schema_version: 1, source: 'UserTurn', session: 'session:configuration-process', agent_profile: product.template.agent_profile,
    input: 'Verify the managed fixture configuration.', budget: null, idempotency_key: runKey } });
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
  const stream = await product.request(`/api/v1/brain/runs/${encodeURIComponent(run)}/events?after=0`, { text: true });
  for (const kind of ['RunAccepted', 'SessionBound', 'ContextBuildFinished', 'ModelCallStarted', 'ModelCallFinished', 'RunComplete']) {
    assert.ok(stream.includes(kind), `runtime trace is missing ${kind}`);
  }
  if (process.env.FORME_CONFIG_BROWSER_PREVIEW === '1') {
    // Explicit local inspection only; CI never pauses. Fixture login is above.
    console.log(`FORME_FIXTURE_BROWSER_URL=${product.origin}`);
    await new Promise((resolve) => { process.stdin.once('data', resolve); process.stdin.resume(); });
    process.stdin.pause();
  }
  // New config is saved while old core continues using its immutable key file.
  const second = save(baseUrl, 1, 'config:process-second');
  second.models[0].credential = 'rotated-fixture-key-not-a-real-secret';
  await product.request('/api/v1/product/config', { method: 'POST', key: second.operation_id, body: second });
  const pending = await product.request('/api/v1/product/config');
  assert.equal(pending.active_revision, 2); assert.equal(pending.acknowledged_revision, 1); assert.equal(pending.pending_restart, true);
  assert.equal(await fs.readFile(coreConfig.brain.models[0].credential_path, 'utf8'), credential);
  await product.stop(core);
  // The fixture deliberately kills the process; respect its original writer lease.
  await new Promise((resolve) => setTimeout(resolve, product.template.authority_lease + 200));
  core = await product.startCore();
  assert.equal((await product.request('/api/v1/product/config')).acknowledged_revision, 2);
  // Acknowledgement means loaded, not that the rotated provider credential works.
  assert.equal((await readJson(product.template.configuration_ack_path)).revision, 2);
  await product.stop(core); await product.stop(web); await product.stop(host);
});
