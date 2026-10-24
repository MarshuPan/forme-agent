// Temporary installed daemons with actual cognition; no user configuration.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { once } from 'node:events';
import { createManagedProduct, readJson, writeJson } from './fixtures/managed_product.mjs';
import { exerciseWebCorrection } from './fixtures/correction_web_process.mjs';

test('owner correction commits through installed daemons and survives a real core restart', {
  timeout: process.env.FORME_CORRECTION_BROWSER_PREVIEW === '1' ? 600000 : 120000,
}, async (t) => {
  const product = await createManagedProduct(); t.after(() => product.cleanup());
  // Use a short explicit fixture cadence so its generated question can surface.
  product.template.cognition_tick = 1000;
  await writeJson(product.host.product_config_template_path, product.template);
  const model = createServer(async (request, response) => {
    for await (const _chunk of request) { /* consume only disposable requests */ }
    response.writeHead(200, { 'Content-Type': 'application/json' });
    response.end(JSON.stringify({ choices: [{ message: { content: '[]' }, finish_reason: 'stop' }],
      usage: { prompt_tokens: 20, completion_tokens: 2 } }));
  });
  model.listen(0, '127.0.0.1'); await once(model, 'listening');
  t.after(() => new Promise((resolve) => { model.close(resolve); model.closeAllConnections(); }));
  await product.startHost(); await product.startWeb();
  const setup = 'claim:correction-process';
  await product.request('/api/v1/bootstrap/prove', { method: 'POST', key: setup,
    body: { schema_version: 1, proof: product.proof(setup) } });
  const candidate = await product.request('/api/v1/bootstrap/data');
  await product.request('/api/v1/bootstrap/data', { method: 'POST', key: setup,
    body: { schema_version: 1, candidate_ref: candidate.candidate_ref } });
  await product.request('/api/v1/bootstrap/claim', { method: 'POST', key: setup, body: {
    schema_version: 1, operation_id: setup, candidate_ref: candidate.candidate_ref,
    login_name: 'fixture', password: 'correction browser fixture password 64',
  } });
  await product.request('/api/v1/product/config', { method: 'POST', key: 'config:correction', body: {
    schema_version: 1, operation_id: 'config:correction', expected_revision: 0, models: [{ schema_version: 1,
      id: 'model:correction-fixture', provider: 'openai-compatible', model: 'correction-fixture',
      base_url: `http://127.0.0.1:${model.address().port}/v1`, credential: 'fixture-only-not-a-real-secret',
      context_window: 128000, tool_use: false, strength: 'standard', input_microunits_per_million: 0,
      output_microunits_per_million: 0, requests_per_minute: 600, tokens_per_minute: 1000000, timeout_ms: 5000,
      fallback_group: null, default: true, tool_schema_versions: [] }],
  } });
  let core = await product.startCore();
  const config = await readJson(product.template.core_config_path);
  const correction = await exerciseWebCorrection(product, config);
  await product.stop(core);
  await new Promise((resolve) => setTimeout(resolve, product.template.authority_lease + 200));
  core = await product.startCore();
  const query = '/api/v1/brain/workbench/corrections/query';
  const result = await product.request(query, { method: 'POST', key: 'correction:restart-query', body: {
    schema_version: 1, command: correction.command.command,
  } });
  assert.deepEqual(result.receipts, [correction.receipt]);
  assert.deepEqual(await product.request('/api/v1/brain/workbench/correction', { method: 'POST',
    key: correction.command.command, body: correction.command }), correction.receipt);
  if (process.env.FORME_CORRECTION_BROWSER_PREVIEW === '1') {
    console.log(`FORME_CORRECTION_FIXTURE_URL=${product.origin}`);
    await new Promise((resolve) => { process.stdin.once('data', resolve); process.stdin.resume(); });
    process.stdin.pause();
  }
  await product.stop(core);
});
