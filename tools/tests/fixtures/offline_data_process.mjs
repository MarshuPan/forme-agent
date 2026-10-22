// Real hostctl processes over a snapshot made by the running ProductionRuntime.
// Only repository-owned temporary data is used; no SCM or active pointer changes.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { promises as fs } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { readJson, writeJson } from './managed_product.mjs';

const executable = fileURLToPath(new URL(`../../../target/debug/forme-hostctl${process.platform === 'win32' ? '.exe' : ''}`, import.meta.url));
const digest = async (file) => createHash('sha256').update(await fs.readFile(file)).digest('hex');
const absent = async (file) => assert.rejects(fs.stat(file), { code: 'ENOENT' });
async function keys(directory) {
  const files = (await fs.readdir(directory)).sort();
  return Promise.all(files.map(async (file) => [file, await digest(path.join(directory, file))]));
}

export async function exerciseOfflineData(product, originalConfig) {
  const environment = { TEMP: product.root, TMP: product.root };
  if (process.env.SystemRoot) environment.SystemRoot = process.env.SystemRoot;
  function cli(command, config, tail = [], success = true) {
    const result = spawnSync(executable, [command, '--config', config, ...tail], {
      cwd: product.root, env: environment, timeout: 30000, maxBuffer: 256 * 1024, windowsHide: true,
      stdio: ['ignore', 'pipe', 'pipe'], encoding: 'utf8',
    });
    assert.equal(result.error, undefined, 'local maintenance process did not finish');
    assert.equal(result.status, success ? 0 : 1, `local maintenance ${command} result differs: ${String(result.stderr).replaceAll(product.root, '[fixture-root]').slice(0, 400)}`);
    assert.equal(result.stdout.includes(product.root), false, 'maintenance output must use opaque digests');
    if (!success) { assert.equal(result.stdout, ''); return; }
    return JSON.parse(result.stdout);
  }
  // The live database is denied before opening a key or initializing SQLite.
  cli('inspect-data', product.template.core_config_path, [], false);
  const liveBackup = 'offline-process-source.sqlite';
  const coreOrigin = `http://${product.coreAddress}`;
  const token = (await fs.readFile(originalConfig.transport.owner_credential_path, 'utf8')).trim();
  const result = await fetch(`${coreOrigin}/v1/data/backup`, { method: 'POST', signal: AbortSignal.timeout(30000),
    headers: { Authorization: `Bearer ${token}`, Origin: coreOrigin, 'X-Forme-Csrf': '1', 'Content-Type': 'application/json' },
    body: JSON.stringify({ schema_version: 1, target: liveBackup }) });
  assert.equal(result.status, 200);
  const manifest = await result.json();
  const source = path.join(path.dirname(originalConfig.brain.store_path), 'data-operations', liveBackup);
  const keyDirectory = originalConfig.brain.content_key_directory
    ?? originalConfig.brain.store_path.replace(/\.[^./\\]+$/, '.content-keys');
  const config = structuredClone(originalConfig);
  config.brain.store_path = source;
  config.brain.content_key_directory = keyDirectory;
  const configPath = path.join(product.root, 'offline-source-config.json');
  await writeJson(configPath, config);
  const sourceBefore = await digest(source);
  const keysBefore = await keys(keyDirectory);
  const identity = cli('inspect-data', configPath);
  assert.equal(identity.database_digest, `sha256:${sourceBefore}`);
  assert.equal(identity.owner_digest, manifest.owner_digest);
  assert.ok(identity.event_count > 0);
  assert.ok(identity.authenticated_content_rows > 0);
  // Disaster recovery does not depend on model, scheduler or transport health.
  const brokenRuntimeConfig = structuredClone(config);
  brokenRuntimeConfig.brain.models = [{ unsupported: 'broken-runtime-only' }];
  brokenRuntimeConfig.brain.scheduler = null;
  brokenRuntimeConfig.transport = null;
  const brokenRuntimePath = path.join(product.root, 'broken-runtime-config.json');
  await writeJson(brokenRuntimePath, brokenRuntimeConfig);
  assert.deepEqual(cli('inspect-data', brokenRuntimePath), identity);
  brokenRuntimeConfig.brain.unknown_storage_override = source;
  await writeJson(brokenRuntimePath, brokenRuntimeConfig);
  cli('inspect-data', brokenRuntimePath, [], false);
  const backup = path.join(product.root, 'offline-backup.sqlite');
  const verification = cli('backup-data', configPath, ['--target', backup]);
  for (const field of ['digest_ok', 'integrity_ok', 'event_continuity_ok', 'projection_replay_ok']) assert.equal(verification[field], true);
  assert.deepEqual(cli('verify-data', configPath, ['--source', backup]), verification);
  cli('backup-data', configPath, ['--target', backup], false);
  const target = path.join(product.root, 'restored-process.sqlite');
  const plan = cli('plan-restore-data', configPath, ['--source', backup, '--target', target]);
  assert.deepEqual(await readJson(`${target}.restore-plan.json`), plan);
  assert.ok(plan.expires_at > Date.now());
  const restoreArguments = ['--source', backup, '--target', target, '--approve'];
  cli('restore-data', configPath, [...restoreArguments, `sha256:${'0'.repeat(64)}`], false);
  await absent(target); await absent(`${target}.restore-receipt.json`);
  const receipt = cli('restore-data', configPath, [...restoreArguments, plan.digest]);
  assert.equal(receipt.source_unchanged, true);
  assert.equal(receipt.authority_switched, false);
  assert.equal(receipt.restored.store_identity, identity.store_identity);
  assert.deepEqual(await readJson(`${target}.restore-receipt.json`), receipt);
  const targetBefore = await digest(target);
  cli('restore-data', configPath, [...restoreArguments, plan.digest], false);
  assert.equal(await digest(target), targetBefore);
  assert.equal(await digest(source), sourceBefore);
  assert.deepEqual(await keys(keyDirectory), keysBefore);
  // Neither a wrong owner nor a missing original vault may mint replacement keys.
  const wrong = structuredClone(config); wrong.brain.owner = 'owner:foreign-fixture';
  const wrongConfig = path.join(product.root, 'wrong-owner.json');
  await writeJson(wrongConfig, wrong); cli('inspect-data', wrongConfig, [], false);
  wrong.brain.owner = config.brain.owner;
  wrong.brain.content_key_directory = path.join(product.root, 'absent-original-keys');
  await writeJson(wrongConfig, wrong); cli('inspect-data', wrongConfig, [], false);
  await absent(wrong.brain.content_key_directory);
  // Opening a restored runtime is explicit and still uses the original vault.
  const restoredConfigPath = path.join(product.root, 'restored-core-config.json');
  config.brain.store_path = target;
  config.brain.expected_restoration_fence = receipt.execution_fence_digest;
  // This explicit test-process configuration is not a Host commit receipt.
  // The managed-product test exercises closed/open admission separately.
  config.transport.work_admission_path = path.join(product.root, 'restored-core.work-admission.json');
  config.transport.configuration_ack_path = path.join(product.root, 'restored-config-ack.json');
  await writeJson(restoredConfigPath, config);
  for (const database of [source, backup, target]) {
    for (const suffix of ['-wal', '-shm', '-journal']) await absent(database + suffix);
  }
  return { restoredConfigPath, identity, receipt, source, run: cli,
    verify: () => cli('verify-data', configPath, ['--source', target]) };
}
