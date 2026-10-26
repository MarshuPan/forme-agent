import assert from 'node:assert/strict';
import { createHash, createPrivateKey, createPublicKey, verify } from 'node:crypto';
import { chmod, mkdtemp, mkdir, readFile, readdir, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';
import { test } from 'node:test';
import { signBundle } from '../sign-product-release.mjs';
import { buildBundle } from '../build-product-bundle.mjs';

const sha = (value) => createHash('sha256').update(value).digest('hex');
const hash = (value) => `sha256:${sha(JSON.stringify(value))}`;

async function fixture(t) {
  const root = await mkdtemp(join(tmpdir(), 'forme-signed-release-'));
  t.after(() => rm(root, { recursive: true, force: true }));
  const bundle = join(root, 'bundle'), output = join(root, 'signed');
  await mkdir(bundle);
  const files = [];
  for (const path of ['bin/forme-hostd.exe', 'bin/forme-webd.exe', 'bin/forme-gatewayd.exe',
    'bin/forme-cli.exe', 'bin/forme-hostctl.exe', 'bin/forme-executord.exe', 'share/forme/web/index.html',
    'share/forme/web/dependencies.json', 'NOTICE', 'LICENSE']) {
    const bytes = Buffer.from(`repository-owned signed fixture ${path}`);
    await mkdir(dirname(join(bundle, path)), { recursive: true });
    await writeFile(join(bundle, path), bytes);
    files.push({ path, bytes: bytes.length, sha256: sha(bytes) });
  }
  const inventory = { schema_version: 1, product: 'forme', release_id: '0.0.2-test',
    target: 'windows-x86_64', development: false, files };
  const inventoryPath = join(bundle, 'bundle-inventory.json');
  await writeFile(inventoryPath, JSON.stringify(inventory));
  const key = createPrivateKey({ key: Buffer.concat([Buffer.from('302e020100300506032b657004220420', 'hex'), Buffer.alloc(32, 0x36)]), type: 'pkcs8', format: 'der' });
  const keyFile = join(root, 'fixture-signing.pem');
  await writeFile(keyFile, key.export({ type: 'pkcs8', format: 'pem' }), { mode: 0o600 });
  const range = { schema_version: 1, read_min: 1, read_max: 2, write_version: 1 };
  const compatibility = { schema_version: 1, config_range: range, data_range: range, minimum_upgrader: 1 };
  const compatibilityFile = join(root, 'compatibility.json');
  await writeFile(compatibilityFile, JSON.stringify(compatibility));
  return { root, key, inventory, inventoryPath, compatibility, config: { bundle, output, keyFile,
    keyId: 'fixture-release-key', compatibilityFile } };
}

test('signed release binds Rust field order, every binary, UI, target and explicit compatibility', async (t) => {
  const fixtureData = await fixture(t);
  const { config, key } = fixtureData;
  const receipt = await signBundle(config);
  const manifest = JSON.parse(await readFile(join(config.output, 'manifest.json')));
  const signature = await readFile(join(config.output, 'signature.ed25519'));
  assert.equal(receipt.release_digest, hash(manifest));
  assert(verify(null, Buffer.from(hash(manifest)), createPublicKey(key), signature));
  assert.equal(signature.length, 64);
  assert.equal(manifest.target, 'windows_x86_64');
  assert.equal(manifest.ui_digest, hash(manifest.files.filter((file) => file.relative_path.startsWith('web/'))));
  assert.equal(manifest.files.filter((file) => file.relative_path.startsWith('bin/')).length, 6);
  for (const file of manifest.files) {
    assert.equal(file.digest, `sha256:${sha(await readFile(join(config.output, 'content', file.relative_path)))}`);
    assert(!file.relative_path.includes('signing.pem'));
  }
  assert.equal(receipt.native_package_signature, 'not_performed');
  assert.equal(receipt.native_installation, 'not_performed');
  const second = await signBundle({ ...config, output: `${config.output}-second` });
  assert.equal(second.release_digest, receipt.release_digest);
  assert.deepEqual(await readFile(join(`${config.output}-second`, 'signature.ed25519')), signature);
});

test('the actual bundle output signs with its exact derived checksum index', async (t) => {
  const { root, config } = await fixture(t);
  const project = join(root, 'project');
  const binaries = join(project, 'target/release');
  const web = join(project, 'crates/gateway/assets/product/dist');
  await mkdir(binaries, { recursive: true });
  await mkdir(web, { recursive: true });
  for (const name of ['forme-hostd', 'forme-webd', 'forme-gatewayd', 'forme-cli', 'forme-hostctl', 'forme-executord']) {
    await writeFile(join(binaries, `${name}.exe`), `original chained fixture ${name}`);
  }
  const webFiles = [];
  for (const path of ['index.html', 'LICENSES.txt', 'dependencies.json']) {
    const value = `original chained Web fixture ${path}`;
    await writeFile(join(web, path), value);
    webFiles.push({ path, sha256: sha(value), bytes: Buffer.byteLength(value) });
  }
  await writeFile(join(web, 'manifest.json'), JSON.stringify({ schema_version: 1, files: webFiles }));
  for (const path of ['LICENSE', 'NOTICE', 'packaging/windows/forme.iss', 'packaging/windows/install-access.ps1', 'packaging/windows/service-access.psm1']) {
    await mkdir(dirname(join(project, path)), { recursive: true });
    await writeFile(join(project, path), `original chained fixture ${path}`);
  }
  const bundle = join(root, 'chained-bundle');
  await buildBundle({ target: 'windows-x86_64', releaseId: '0.0.3', binaryDir: binaries, output: bundle, development: false }, project);
  const receipt = await signBundle({ ...config, bundle });
  assert.equal(receipt.native_installation, 'not_performed');
  const sums = join(bundle, 'SHA256SUMS');
  await writeFile(sums, `${await readFile(sums, 'utf8')}unlisted-index-line\n`);
  const badOutput = join(root, 'tampered-index-output');
  await assert.rejects(signBundle({ ...config, bundle, output: badOutput }));
  await assert.rejects(readdir(badOutput), { code: 'ENOENT' });
});

test('no output is created for tampered inventory, escaping paths, extra files or bad compatibility', async (t) => {
  for (const kind of ['tamper', 'escape', 'extra', 'compatibility', 'target', 'missing', 'duplicate']) {
    const { config, inventory, inventoryPath, compatibility } = await fixture(t);
    if (kind === 'tamper') await writeFile(join(config.bundle, 'bin/forme-hostd.exe'), 'different bytes');
    if (kind === 'escape') inventory.files[0].path = '../outside';
    if (kind === 'extra') await writeFile(join(config.bundle, 'unreviewed'), 'extra');
    if (kind === 'compatibility') { compatibility.data_range.write_version = 10; await writeFile(config.compatibilityFile, JSON.stringify(compatibility)); }
    if (kind === 'target') inventory.target = 'guessed-platform';
    if (kind === 'missing') inventory.files.splice(0, 1);
    if (kind === 'duplicate') inventory.files.push(inventory.files[0]);
    await writeFile(inventoryPath, JSON.stringify(inventory));
    await assert.rejects(signBundle(config));
    await assert.rejects(readFile(join(config.output, 'manifest.json')), { code: 'ENOENT' });
  }
});

test('private signing key never enters source/output and existing output is retained', async (t) => {
  const { config } = await fixture(t);
  await assert.rejects(signBundle({ ...config, keyFile: join(config.bundle, 'key.pem') }));
  await mkdir(config.output);
  await writeFile(join(config.output, 'keep'), 'old output');
  await assert.rejects(signBundle(config));
  assert.deepEqual(await readdir(config.output), ['keep']);
  assert.equal(await readFile(join(config.output, 'keep'), 'utf8'), 'old output');
  if (process.platform !== 'win32') {
    const { config: broad } = await fixture(t);
    await chmod(broad.keyFile, 0o644);
    await assert.rejects(signBundle(broad));
  }
});

test('the real Rust release verifier accepts the offline signature and rejects replaced content', async (t) => {
  const binary = process.env.FORME_RELEASE_VERIFIER;
  if (!binary) return t.skip('requires an explicitly built forme-hostd; unit evidence only');
  assert.equal(resolve(binary), binary);
  const { root, config } = await fixture(t);
  const receipt = await signBundle(config);
  const trust = join(root, 'administrator-trust.json');
  await writeFile(trust, JSON.stringify({ schema_version: 1, install_ref: 'fixture-installation',
    pinned_keys: { [config.keyId]: receipt.public_key } }));
  const args = ['--verify-release', config.output, '--trust-config', trust, '--target', 'windows_x86_64'];
  const result = spawnSync(binary, args, { encoding: 'utf8', timeout: 20_000, windowsHide: true, env: {} });
  assert.equal(result.status, 0, result.stderr);
  assert.equal(JSON.parse(result.stdout).release_digest, receipt.release_digest);
  assert.equal(JSON.parse(result.stdout).native_installation, 'not_performed');
  await writeFile(join(config.output, 'content/web/index.html'), 'replacement');
  const replaced = spawnSync(binary, args, { encoding: 'utf8', timeout: 20_000, windowsHide: true, env: {} });
  assert.notEqual(replaced.status, 0);
  assert(!replaced.stdout.includes(receipt.release_digest));
});
