// A second Web daemon must not bypass the singleton authentication writer.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createManagedProduct } from './fixtures/managed_product.mjs';

const password = 'multi-process web fixture password 83';
const save = (baseUrl, revision, operation) => ({ schema_version: 1, operation_id: operation,
  expected_revision: revision, models: [{ schema_version: 1, id: 'model:multi-process-fixture',
    provider: 'openai-compatible', model: 'multi-process-fixture-model', base_url: baseUrl,
    credential: 'fixture-multi-process-key-not-a-real-secret', context_window: 128000,
    tool_use: false, strength: 'standard', input_microunits_per_million: 0,
    output_microunits_per_million: 0, requests_per_minute: 60, tokens_per_minute: 100000,
    timeout_ms: 1000, fallback_group: null, default: true, tool_schema_versions: [] }] });

test('Web singleton contention fails closed and restart recovery preserves owner state', {
  timeout: 180000,
}, async (t) => {
  const product = await createManagedProduct();
  t.after(() => product.cleanup());
  const host = await product.startHost();
  t.after(() => product.stop(host));
  let primaryWeb = await product.startWeb();
  const setup = 'claim:web-multi-process';
  await product.request('/api/v1/bootstrap/prove', { method: 'POST', key: setup,
    body: { schema_version: 1, proof: product.proof(setup) } });
  const candidate = await product.request('/api/v1/bootstrap/data');
  await product.request('/api/v1/bootstrap/data', { method: 'POST', key: setup,
    body: { schema_version: 1, candidate_ref: candidate.candidate_ref } });
  await product.request('/api/v1/bootstrap/claim', { method: 'POST', key: setup,
    body: { schema_version: 1, operation_id: setup, candidate_ref: candidate.candidate_ref,
      login_name: 'fixture', password } });

  const secondaryConfig = await product.cloneWebConfig();
  await assert.rejects(
    product.startWebConfig(secondaryConfig),
    /local maintenance|authentication storage/i,
  );

  const primarySession = await product.request('/api/v1/auth/session');
  const initial = await product.request('/api/v1/product/config');
  const operation = 'config:web-singleton-contention';
  const saved = await product.request('/api/v1/product/config', { method: 'POST', key: operation,
    body: save('http://127.0.0.1:9/v1', initial.active_revision, operation) });
  assert.equal(saved.view.active_revision, initial.active_revision + 1);

  await product.stop(primaryWeb);
  primaryWeb = await product.startWeb();
  const afterRestart = await product.request('/api/v1/auth/session');
  assert.equal(afterRestart.session.owner, primarySession.session.owner);
  const afterRestartConfig = await product.request('/api/v1/product/config');
  assert.equal(afterRestartConfig.active_revision, saved.view.active_revision);
  assert.equal(afterRestartConfig.models[0].id, saved.view.models[0].id);

  console.log('Web multi-process: PASS (singleton contention rejection, owner/config restart recovery)');
  console.log('Native service, installer, real provider, and IP01-IP24: NOT_RUN');
});
