// Actual bounded hostctl pipe protocol. No native service-manager simulation.
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { createHash } from 'node:crypto';
import { promises as fs } from 'node:fs';
import { once } from 'node:events';
import { createInterface } from 'node:readline';
import { fileURLToPath } from 'node:url';
import path from 'node:path';
import { readJson, writeJson } from './managed_product.mjs';

const executable = fileURLToPath(new URL(`../../../target/debug/forme-hostctl${process.platform === 'win32' ? '.exe' : ''}`, import.meta.url));
const digest = (value) => `sha256:${createHash('sha256').update(JSON.stringify(value)).digest('hex')}`;

export async function exerciseRestoreWorker(product, offline, password) {
  // This fixture kills its core process, unlike a native graceful SCM stop.
  // Exercise the actual worker against the runtime's cold snapshot; never
  // checkpoint, rewrite or silently recover the crashed original database.
  const original = await fs.readFile(product.template.core_config_path);
  const config = JSON.parse(original);
  config.brain.store_path = offline.source;
  config.brain.content_key_directory = product.template.brain_store_path.replace(/\.[^./\\]+$/, '.content-keys');
  await writeJson(product.template.core_config_path, config);
  try { return await exerciseColdRestoreWorker(product, offline, password); }
  finally { await fs.writeFile(product.template.core_config_path, original); }
}

async function exerciseColdRestoreWorker(product, offline, password) {
  const config = await readJson(product.template.core_config_path);
  const candidate = path.join(path.dirname(product.template.brain_store_path), 'data-operations', 'host-worker-candidate.db');
  const manifest = await readJson(path.join(path.dirname(product.prepared.host_config_path), 'installation.json'));
  const source = path.join(manifest.paths.backups, 'host-worker-source.db');
  offline.run('backup-data', product.template.core_config_path, ['--target', source]);
  const plan = offline.run('plan-restore-data', product.template.core_config_path, ['--source', source, '--target', candidate]);
  const receipt = offline.run('restore-data', product.template.core_config_path, ['--source', source, '--target', candidate, '--approve', plan.digest]);
  const request = { schema_version: 1, install_ref: manifest.install_id, operation_id: 'restore:pipe-process',
    command_digest: digest('explicit-local-worker-fixture'), expected_config_revision: config.revision,
    owner: config.brain.owner, candidate_ref: path.basename(candidate), candidate_digest: digest(receipt.verification.manifest) };
  const environment = { TEMP: product.root, TMP: product.root };
  if (process.env.SystemRoot) environment.SystemRoot = process.env.SystemRoot;
  const child = spawn(executable, ['maintenance-restore', '--installation-manifest', path.join(manifest.paths.config, 'installation.json')], {
    cwd: product.root, env: environment, windowsHide: true, stdio: ['pipe', 'pipe', 'pipe'],
  });
  // Never emit stderr: it is a private test channel, not product evidence.
  child.stderr.resume();
  const lines = createInterface({ input: child.stdout, crlfDelay: Infinity });
  const iterator = lines[Symbol.asyncIterator]();
  const exited = once(child, 'exit');
  let timer;
  const limit = new Promise((_, reject) => { timer = setTimeout(() => { child.kill(); reject(new Error('restore worker protocol timeout')); }, 30000); });
  async function receive(state) {
    const line = await Promise.race([iterator.next(), limit]);
    assert.equal(line.done, false, 'worker exited without the exact response');
    assert.ok(Buffer.byteLength(line.value) <= 65536);
    const response = JSON.parse(line.value);
    assert.equal(response.operation_id, request.operation_id);
    assert.equal(response.command_digest, request.command_digest);
    assert.equal(response.result.state, state);
    return response.result;
  }
  async function step(step, state) {
    child.stdin.write(JSON.stringify({ schema_version: 1, operation_id: request.operation_id, command_digest: request.command_digest, step }) + '\n');
    return receive(state);
  }
  try {
    child.stdin.write(JSON.stringify(request) + '\n');
    const validated = await receive('validated');
    assert.equal(validated.binding.manifest_digest, request.candidate_digest);
    assert.equal(validated.binding.execution_fence_digest, receipt.execution_fence_digest);
    offline.run('inspect-data', product.template.core_config_path, [], false);
    const backup = await step('backup', 'backed_up');
    const verification = offline.run('verify-data', product.template.core_config_path, ['--source', path.join(manifest.paths.backups, backup.backup_ref)]);
    assert.equal(digest(verification.manifest), backup.manifest_digest);
    const fenced = await step('fence_sessions', 'sessions_fenced');
    assert.ok(fenced.revision.auth_epoch > 1);
    await product.request('/api/v1/auth/session', { expected: 401 });
    await step('release', 'released'); child.stdin.end();
    const [code] = await Promise.race([exited, limit]); assert.equal(code, 0);
    offline.run('inspect-data', product.template.core_config_path);
    // Web stayed live; the same password/owner can sign in after global fencing.
    const challenge = await product.request('/api/v1/auth/login-challenge');
    const login = await product.request('/api/v1/auth/login', { method: 'POST', key: challenge.attempt_ref,
      body: { schema_version: 1, login_name: 'fixture', password } });
    assert.equal(login.session.owner, config.brain.owner);
    return { binding: validated.binding, request };
  } finally {
    clearTimeout(timer); lines.close();
    if (child.exitCode === null && child.signalCode === null) { child.kill(); await exited; }
  }
}

export async function setFixtureAdmission(product, offline, released) {
  const config = await readJson(offline.restoredConfigPath);
  await writeJson(config.transport.work_admission_path, { schema_version: 1, install_ref: config.install_ref,
    operation_id: 'restore:test-process-admission', command_digest: digest('test-process-only-not-a-host-commit'),
    config_revision: config.revision, owner_digest: offline.identity.owner_digest,
    execution_fence_digest: offline.receipt.execution_fence_digest, released });
}
