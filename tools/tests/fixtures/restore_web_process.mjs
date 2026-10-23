// Real authenticated Web -> ProductionRuntime restoration. No native switching.
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import path from 'node:path';
import { readJson } from './managed_product.mjs';

export async function exerciseWebRestoreCandidate(product, password, coreConfig) {
  const command = async (key, action) => product.request('/api/v1/data/command', {
    method: 'POST', key, body: { command: { schema_version: 1, command: key, action }, password },
  });
  const source = 'web-candidate-source.sqlite', target = 'web-restoration-candidate.sqlite';
  const original = await product.request('/api/v1/product/config');
  const backup = await command('backup:web-candidate', { Backup: { target: source } });
  assert.equal(backup.state, 'Confirmed');
  assert.equal(backup.backup.restoration, undefined);
  const prepared = await command('prepare:web-candidate', { PrepareRestore: { source, target } });
  assert.equal(prepared.state, 'Prepared');
  assert.equal(prepared.backup, null);
  const applied = await command('apply:web-candidate', {
    Apply: { prepared_by: prepared.command, plan_digest: prepared.plan.digest },
  });
  assert.equal(applied.state, 'Confirmed');
  assert.equal(applied.backup.verified, true);
  const candidate = applied.backup.restoration;
  assert.equal(candidate.schema_version, 1);
  assert.equal(candidate.candidate_ref, target);
  for (const field of ['candidate_digest', 'owner_digest', 'store_identity', 'execution_fence_digest']) {
    assert.match(candidate[field], /^sha256:[0-9a-f]{64}$/);
  }
  const finalManifest = await readJson(path.join(path.dirname(coreConfig.brain.store_path), 'data-operations', `${target}.manifest.json`));
  const digest = `sha256:${createHash('sha256').update(JSON.stringify(finalManifest)).digest('hex')}`;
  assert.equal(candidate.candidate_digest, digest);
  assert.notEqual(candidate.candidate_digest, finalManifest.database_digest);
  assert.equal(candidate.owner_digest, finalManifest.owner_digest);
  assert.equal(JSON.stringify(applied).includes(product.root), false);
  assert.equal((await product.request('/api/v1/product/config')).active_revision, original.active_revision);
  assert.equal((await readJson(product.template.core_config_path)).brain.store_path, coreConfig.brain.store_path);
  const query = { schema_version: 1, query: { Inspect: { command: applied.command } } };
  const observed = await product.request('/api/v1/data/query', { method: 'POST', body: query });
  assert.deepEqual(observed.receipt, applied);
  const verification = await product.request('/api/v1/data/query', {
    method: 'POST', body: { schema_version: 1, query: { VerifyBackup: { source: target } } },
  });
  assert.equal(verification.verified, true);
  assert.equal(verification.restoration, undefined, 'verification alone cannot authorize activation');
  return applied;
}
