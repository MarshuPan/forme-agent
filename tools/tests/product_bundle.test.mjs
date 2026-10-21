import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { mkdtemp, mkdir, readFile, readdir, rm, symlink, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { test } from 'node:test';
import { buildBundle, parseArguments, targets } from '../build-product-bundle.mjs';

const sha = (value) => createHash('sha256').update(value).digest('hex');

async function fixture(t, target = 'windows-x86_64') {
  const scratch = await mkdtemp(join(tmpdir(), 'forme-bundle-test-'));
  t.after(() => rm(scratch, { recursive: true, force: true }));
  const root = join(scratch, 'repo'), binaryDir = join(root, 'target/release');
  const output = join(scratch, 'result');
  const assets = join(root, 'crates/gateway/assets/product/dist');
  await mkdir(binaryDir, { recursive: true }); await mkdir(assets, { recursive: true });
  for (const file of ['forme-hostd', 'forme-webd', 'forme-gatewayd', 'forme-cli', 'forme-hostctl']) {
    await writeFile(join(binaryDir, file + targets[target].extension), `repository-owned fixture for ${file}`, { mode: 0o755 });
  }
  const files = [];
  for (const [path, content] of [['index.html', '<main>test</main>'], ['LICENSES.txt', 'fixture license'], ['dependencies.json', '{}'], ['assets/test.js', 'console.log("test")']]) {
    await mkdir(dirname(join(assets, path)), { recursive: true });
    await writeFile(join(assets, path), content);
    files.push({ path, bytes: Buffer.byteLength(content), sha256: sha(content) });
  }
  await writeFile(join(assets, 'manifest.json'), JSON.stringify({ schema_version: 1, files }));
  await writeFile(join(root, 'LICENSE'), 'repository test license');
  await writeFile(join(root, 'NOTICE'), 'repository test notice');
  const template = join(root, 'packaging', targets[target].template);
  await mkdir(template, { recursive: true });
  const required = {
    windows: ['forme.iss'],
    linux: ['forme-brain.service', 'forme-host.service', 'forme-web.service', 'forme.control', 'forme.postinst', 'forme.spec'],
    macos: ['Distribution.xml', 'com.forme.brain.plist', 'com.forme.host.plist', 'com.forme.web.plist', 'scripts/postinstall'],
  }[targets[target].template];
  for (const path of required) {
    const destination = join(template, path);
    await mkdir(dirname(destination), { recursive: true });
    await writeFile(destination, `repository-owned packaging input: ${path}`, { mode: path.endsWith('postinstall') ? 0o755 : 0o644 });
  }
  if (targets[target].template === 'macos') {
    await mkdir(join(template, 'scripts'), { recursive: true });
    await writeFile(join(template, 'scripts', 'postinstall'), '#!/bin/sh\nexit 0\n', { mode: 0o755 });
  }
  return { root, assets, scratch, config: { target, releaseId: '0.0.1-test', binaryDir, output, development: false } };
}

test('bundle arguments reject duplicates, unknown targets, relative paths and implicit debug use', () => {
  const args = ['--target', 'windows-x86_64', '--release-id', '0.0.1', '--binary-dir', join(tmpdir(), 'release'), '--output', join(tmpdir(), 'fresh-output')];
  assert.equal(parseArguments(args).development, false);
  for (const change of [
    [...args, '--target', 'windows-x86_64'], [...args, '--unknown'],
    [...args.slice(0, -1), 'relative'], [...args.slice(0, -1)],
  ]) assert.throws(() => parseArguments(change));
  const debug = args.map((arg) => arg === join(tmpdir(), 'release') ? join(tmpdir(), 'debug') : arg);
  assert.throws(() => parseArguments(debug));
  assert.equal(parseArguments([...debug, '--development']).development, true);
});

test('bundle hashes every reviewed file reproducibly and records unverified release boundaries', async (t) => {
  for (const target of Object.keys(targets)) {
    const { root, config } = await fixture(t, target);
    const inventory = await buildBundle(config, root);
    assert.equal(inventory.release_ready, false);
    assert.equal(inventory.signature, 'not_signed');
    assert.equal(inventory.native_installation, 'not_performed');
    assert.equal(inventory.binary_target_verification, 'not_performed');
    assert.equal(inventory.files.filter((f) => f.path.startsWith('bin/')).length, 5);
    assert(inventory.files.some((f) => f.path.endsWith('LICENSES.txt')));
    if (targets[target].template === 'macos') {
      assert(inventory.files.some((f) => f.path === 'packaging/macos/scripts/postinstall'));
      assert(inventory.files.some((f) => f.path === 'packaging/macos/Distribution.xml'));
      assert(inventory.files.some((f) => f.path === 'packaging/macos/com.forme.brain.plist'));
    }
    for (const file of inventory.files) {
      const bytes = await readFile(join(config.output, file.path));
      assert.equal(bytes.length, file.bytes); assert.equal(sha(bytes), file.sha256);
    }
    const second = await buildBundle({ ...config, output: `${config.output}-second` }, root);
    assert.equal(inventory.content_digest, second.content_digest);
  }
});

test('bundle never replaces an existing output or writes before preflight succeeds', async (t) => {
  const { root, config } = await fixture(t);
  await mkdir(config.output);
  await writeFile(join(config.output, 'preserve'), 'keep');
  await assert.rejects(buildBundle(config, root), { code: 'EEXIST' });
  assert.deepEqual(await readdir(config.output), ['preserve']);
  assert.equal(await readFile(join(config.output, 'preserve'), 'utf8'), 'keep');
  await rm(join(config.binaryDir, 'forme-hostctl.exe'));
  const fresh = `${config.output}-missing-binary`;
  await assert.rejects(buildBundle({ ...config, output: fresh }, root));
  await assert.rejects(readFile(join(fresh, 'bundle-inventory.json')), { code: 'ENOENT' });
});

test('altered or undeclared Web assets and escaping paths are rejected', async (t) => {
  for (const alteration of ['tamper', 'extra', 'traversal', 'missing-license']) {
    const { root, assets, config } = await fixture(t);
    if (alteration === 'tamper') await writeFile(join(assets, 'index.html'), 'altered');
    if (alteration === 'extra') await writeFile(join(assets, 'secret.env'), 'fixture, never owner data');
    if (['traversal', 'missing-license'].includes(alteration)) {
      const manifest = JSON.parse(await readFile(join(assets, 'manifest.json'), 'utf8'));
      if (alteration === 'traversal') manifest.files[0].path = '../escape';
      else manifest.files = manifest.files.filter((f) => f.path !== 'LICENSES.txt');
      await writeFile(join(assets, 'manifest.json'), JSON.stringify(manifest));
    }
    await assert.rejects(buildBundle(config, root));
    await assert.rejects(readdir(config.output), { code: 'ENOENT' });
  }
});

test('bundle refuses repo roots, input overlap and filesystem indirection', async (t) => {
  const { root, scratch, config } = await fixture(t);
  for (const output of [root, config.binaryDir, join(root, 'frontend'), join(root, 'target/product-bundles')]) {
    await assert.rejects(buildBundle({ ...config, output }, root));
  }
  const linked = join(scratch, 'linked-input');
  await symlink(config.binaryDir, linked, process.platform === 'win32' ? 'junction' : 'dir');
  await assert.rejects(buildBundle({ ...config, binaryDir: linked }, root), /Linked paths/);
});

test('bundle refuses an incomplete native packaging template', async (t) => {
  const { root, config } = await fixture(t, 'macos-arm64');
  await rm(join(root, 'packaging', 'macos', 'scripts', 'postinstall'));
  await assert.rejects(buildBundle(config, root), /missing required input/);
  await assert.rejects(readdir(config.output), { code: 'ENOENT' });
});
