import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { mkdtemp, mkdir, readFile, readdir, rm, symlink, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { buildBundle, parseArguments, targets } from '../build-product-bundle.mjs';

const sha = (value) => createHash('sha256').update(value).digest('hex');
const repositoryRoot = join(dirname(fileURLToPath(import.meta.url)), '../..');

async function fixture(t, target = 'windows-x86_64') {
  const scratch = await mkdtemp(join(tmpdir(), 'forme-bundle-test-'));
  t.after(() => rm(scratch, { recursive: true, force: true }));
  const root = join(scratch, 'repo'), binaryDir = join(root, 'target/release');
  const output = join(scratch, 'result');
  const assets = join(root, 'crates/gateway/assets/product/dist');
  await mkdir(binaryDir, { recursive: true }); await mkdir(assets, { recursive: true });
  for (const file of ['forme-hostd', 'forme-webd', 'forme-gatewayd', 'forme-cli', 'forme-hostctl', 'forme-executord', 'forme-file-worker', 'forme-pty-worker']) {
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
    windows: ['forme.iss', 'install-access.ps1', 'service-access.psm1'],
    linux: ['forme-brain.service', 'forme-host.service', 'forme-web.service', 'forme.control', 'forme.preinst', 'forme.postinst', 'forme.prerm', 'forme.spec'],
    macos: ['Distribution.xml', 'com.forme.brain.plist', 'com.forme.host.plist', 'com.forme.web.plist', 'scripts/preinstall', 'scripts/postinstall'],
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
  if (targets[target].template !== 'windows') {
    await mkdir(join(root, 'packaging/posix'), { recursive: true });
    await writeFile(join(root, 'packaging/posix/service-access.sh'), '#!/bin/sh\nexit 0\n', { mode: 0o755 });
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
    assert.equal(inventory.files.filter((f) => f.path.startsWith('bin/')).length, 8);
    assert.ok(inventory.files.some((f) => f.path === `bin/forme-executord${targets[target].extension}`));
    const declarationPath = 'share/forme/desktop-output-v2.json';
    if (target === 'windows-x86_64') {
      const declaration = JSON.parse(await readFile(join(config.output, declarationPath), 'utf8'));
      const helper = inventory.files.find((file) => file.path === 'bin/forme-executord.exe');
      assert.deepEqual(declaration, { schema_version: 2,
        helper_digest: `sha256:${helper.sha256}`, helper_bytes: helper.bytes });
      assert.ok(inventory.files.some((file) => file.path === declarationPath));
    } else {
      assert.ok(!inventory.files.some((file) => file.path === declarationPath));
    }
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

test('a new complete bundle refuses a missing one-shot execution image before any output', async (t) => {
  for (const name of ['forme-executord.exe', 'forme-file-worker.exe', 'forme-pty-worker.exe']) {
    const { root, config } = await fixture(t);
    await rm(join(config.binaryDir, name));
    await assert.rejects(buildBundle(config, root));
    await assert.rejects(readFile(join(config.output, 'bundle-inventory.json')), { code: 'ENOENT' });
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

test('Windows bundle refuses to omit either access-control helper before writing', async (t) => {
  for (const name of ['install-access.ps1', 'service-access.psm1']) {
    const { root, config } = await fixture(t);
    await rm(join(root, 'packaging/windows', name));
    await assert.rejects(buildBundle(config, root), /missing required input/);
    await assert.rejects(readdir(config.output), { code: 'ENOENT' });
  }
});

test('Windows installer registers and removes all three resident services', async () => {
  const template = await readFile(join(repositoryRoot, 'packaging/windows/forme.iss'), 'utf8');
  for (const [binary, config] of [
    ['forme-hostd', 'host.json'],
    ['forme-webd', 'web.json'],
    ['forme-gatewayd', 'core\\core.json'],
  ]) {
    const escaped = config.replaceAll('\\', '\\\\').replaceAll('.', '\\.');
    assert.match(template, new RegExp(`RequireStep\\(ExpandConstant\\('\\{app\\}\\\\bin\\\\${binary}\\.exe'\\),\\s*'--install-service --config .*${escaped}`));
    assert.match(template, new RegExp(`RunChecked\\(ExpandConstant\\('\\{app\\}\\\\bin\\\\${binary}\\.exe'\\),\\s*'--uninstall-service --config .*${escaped}`));
  }
  const install = template.slice(template.indexOf('procedure CurStepChanged'));
  const stages = ['prepare installation', 'register Host', 'register Web', 'register Brain',
    "AccessArguments('Grant')", "AccessArguments('Verify')", "AccessArguments('Start')"];
  let previous = -1;
  for (const stage of stages) { const index = install.indexOf(stage); assert.ok(index > previous); previous = index; }
  assert.match(template, /Result := Result and \(ExitCode = 0\)/);
  assert.match(template, /if not RunChecked[\s\S]*?RaiseException/);
  assert.match(template, /function GetCustomSetupExitCode\(\): Integer;[\s\S]*?if InstallStagesCompleted then Result := 0 else Result := 1;/);
  assert.ok(install.indexOf('InstallStagesCompleted := True;') > install.indexOf("AccessArguments('Start')"));
  assert.ok(template.indexOf("AccessArguments('Prepare')") < template.indexOf('procedure CurStepChanged'));
  assert.doesNotMatch(template.slice(template.indexOf('[Run]'), template.indexOf('[Code]')), /install-service/);
  assert.doesNotMatch(template, /\[UninstallRun\]/);
  const uninstall = template.slice(template.indexOf('function InitializeUninstall'));
  assert.match(uninstall, /if Result then\s*Result := RunChecked/g);
  assert.match(uninstall, /if not Result then\s*MsgBox/);
  assert.ok(uninstall.indexOf('remove Web service') < uninstall.indexOf('remove Brain service'));
  assert.ok(uninstall.indexOf('remove Brain service') < uninstall.indexOf('remove Host service'));
});

test('native brain launch paths agree with the partitioned published configuration', async () => {
  const linux = await readFile(join(repositoryRoot, 'packaging/linux/forme-brain.service'), 'utf8');
  assert.match(linux, /^ConditionPathExists=\/etc\/forme\/core\/core\.json$/m);
  assert.match(linux, /^ExecStart=.* --config \/etc\/forme\/core\/core\.json$/m);
  const macos = await readFile(join(repositoryRoot, 'packaging/macos/com.forme.brain.plist'), 'utf8');
  assert.ok(macos.includes('<string>/Library/Application Support/ForMe/config/core/core.json</string>'));
  assert.ok(!linux.includes('/etc/forme/core.json') && !macos.includes('/config/core.json'));
});
