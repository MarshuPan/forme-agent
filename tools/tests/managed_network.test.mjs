// Real isolated Web/Host processes; never controls installed services/firewall.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createManagedProduct, readJson } from './fixtures/managed_product.mjs';

test('Web network settings stage and recover without changing the live listener or invoking a model', { timeout: 120000 }, async (t) => {
  const product = await createManagedProduct();
  t.after(() => product.cleanup());
  let host = await product.startHost();
  await product.startWeb();
  const setup = 'claim:network-process';
  await product.request('/api/v1/bootstrap/prove', { method: 'POST', key: setup,
    body: { schema_version: 1, proof: product.proof(setup) } });
  const candidate = await product.request('/api/v1/bootstrap/data');
  await product.request('/api/v1/bootstrap/data', { method: 'POST', key: setup,
    body: { schema_version: 1, candidate_ref: candidate.candidate_ref } });
  await product.request('/api/v1/bootstrap/claim', { method: 'POST', key: setup, body: {
    schema_version: 1, operation_id: setup, candidate_ref: candidate.candidate_ref,
    login_name: 'fixture', password: 'network process fixture password 72',
  } });
  const original = await readJson(product.template.web_network_config_path);
  const desired = { ...original, bind: '127.0.0.1:1' };
  const save = { schema_version: 1, operation_id: 'network:process-save', expected_revision: 0,
    models: [{ schema_version: 1, id: 'model:unused-network-fixture', provider: 'openai-compatible',
      model: 'network-no-provider', base_url: 'http://127.0.0.1:1/v1', credential: 'fixture-not-real',
      context_window: 128000, tool_use: false, strength: 'standard', input_microunits_per_million: 0,
      output_microunits_per_million: 0, requests_per_minute: 60, tokens_per_minute: 100000,
      timeout_ms: 1000, fallback_group: null, default: true, tool_schema_versions: [] }], network: desired };
  const saved = await product.request('/api/v1/product/config', { method: 'POST', key: save.operation_id, body: save });
  assert.equal(saved.view.active_revision, 1);
  assert.equal(saved.view.network_active, false);
  assert.deepEqual(saved.view.network, desired);
  assert.deepEqual(await readJson(product.template.web_network_config_path), original);
  const validation = await product.request('/api/v1/product/config/validate', { method: 'POST', key: 'network:process-validate',
    body: { schema_version: 1, revision: 1 } });
  assert.equal(validation.model_test_status, 'NotRun');
  assert.match(validation.network_validation_digest, /^sha256:[a-f0-9]{64}$/);
  const invalid = { ...save, operation_id: 'network:invalid-plaintext', expected_revision: 1,
    network: { ...desired, bind: '192.0.2.10:8765' } };
  await product.request('/api/v1/product/config', { method: 'POST', key: invalid.operation_id, body: invalid, expected: 422 });
  assert.equal((await product.request('/api/v1/product/config')).active_revision, 1);
  // Restart only the disposable Host process, never SCM/systemd/launchd.
  await product.stop(host);
  host = await product.startHost();
  const recovered = await product.request('/api/v1/product/config');
  assert.deepEqual(recovered.network, desired);
  assert.equal(recovered.network_active, false);
  assert.deepEqual(await readJson(product.template.web_network_config_path), original);
  const history = await product.request('/api/v1/product/maintenance');
  assert.deepEqual(history.operations, []);
  assert.equal((await product.request('/api/v1/auth/session')).session.owner, recovered.owner);
});
