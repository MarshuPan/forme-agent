// Repository-owned binary/packaging fixtures. Never native installation proof.
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { link, mkdtemp, mkdir, readFile, readdir, rm, writeFile } from 'node:fs/promises';
import { dirname, join, resolve } from 'node:path';
import { tmpdir } from 'node:os';
import { fileURLToPath } from 'node:url';
import { test } from 'node:test';
import { buildBundle } from '../build-product-bundle.mjs';
import { assertHost, buildNativePackage, parseArguments } from '../build-native-package.mjs';
import { binaryMatches, targetDetails, verifyBundle, verifyToolchain, recheckTool } from '../native-package/inputs.mjs';
import { nativeCommands, prepareLayout } from '../native-package/layout.mjs';
import { fileWorkerPe } from './fixtures/file_worker_pe.mjs';

const repository = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const sha = (bytes) => createHash('sha256').update(bytes).digest('hex');

function binary(target) {
  const detail = targetDetails[target];
  if (detail.platform === 'win32') return fileWorkerPe();
  const bytes = Buffer.alloc(256);
  if (detail.platform === 'win32') {
    bytes.write('MZ'); bytes.writeUInt32LE(128, 60); bytes.writeUInt32LE(0x4550, 128);
    bytes.writeUInt16LE(0x8664, 132); bytes.writeUInt16LE(2, 150); bytes.writeUInt16LE(0x20b, 152);
  } else if (detail.platform === 'linux') {
    bytes.write('7f454c46020101', 'hex'); bytes.writeUInt16LE(3, 16);
    bytes.writeUInt16LE(detail.arch === 'arm64' ? 183 : 62, 18); bytes.writeUInt32LE(1, 20);
  } else {
    bytes.writeUInt32LE(0xfeedfacf, 0); bytes.writeUInt32LE(detail.arch === 'arm64' ? 0x100000c : 0x1000007, 4); bytes.writeUInt32LE(2, 12);
  }
  return bytes;
}

async function fixture(t, target = 'windows-x86_64') {
  const scratch = await mkdtemp(join(tmpdir(), 'forme-native-inputs-'));
  t.after(() => rm(scratch, { recursive: true, force: true }));
  const project = join(scratch, 'project'), binaries = join(project, 'target/release');
  const web = join(project, 'crates/gateway/assets/product/dist');
  await mkdir(binaries, { recursive: true }); await mkdir(web, { recursive: true });
  for (const name of ['forme-hostd', 'forme-webd', 'forme-gatewayd', 'forme-cli', 'forme-hostctl', 'forme-executord', 'forme-file-worker']) await writeFile(join(binaries, name + (target.startsWith('windows') ? '.exe' : '')), binary(target));
  const files = [];
  for (const path of ['index.html', 'LICENSES.txt', 'dependencies.json']) {
    const bytes = Buffer.from(`original package fixture ${path}`);
    await writeFile(join(web, path), bytes); files.push({ path, bytes: bytes.length, sha256: sha(bytes) });
  }
  await writeFile(join(web, 'manifest.json'), JSON.stringify({ schema_version: 1, files }));
  for (const path of ['LICENSE', 'NOTICE']) await writeFile(join(project, path), `original ${path} fixture`);
  const platform = target.startsWith('windows') ? 'windows' : target.startsWith('macos') ? 'macos' : 'linux';
  const inputs = platform === 'windows' ? ['forme.iss', 'install-access.ps1', 'service-access.psm1'] : platform === 'linux' ?
    ['forme-brain.service', 'forme-host.service', 'forme-web.service', 'forme.control', 'forme.preinst', 'forme.postinst', 'forme.prerm', 'forme.spec'] :
    ['Distribution.xml', 'com.forme.host.plist', 'com.forme.web.plist', 'com.forme.brain.plist', 'scripts/preinstall', 'scripts/postinstall'];
  // Packaging implementation is the actual repository input, fixture payloads
  // and compiler headers are original to this test and are never executed.
  for (const path of [...inputs.map((name) => `${platform}/${name}`), ...(platform === 'windows' ? [] : ['posix/service-access.sh'])]) {
    const targetPath = join(project, 'packaging', path);
    await mkdir(dirname(targetPath), { recursive: true });
    await writeFile(targetPath, await readFile(join(repository, 'packaging', path)));
  }
  const bundle = join(scratch, 'bundle');
  const inventory = await buildBundle({ target, releaseId: '0.4.0-test', binaryDir: binaries, output: bundle, development: false }, project);
  const toolPath = join(scratch, 'pinned-compiler-fixture');
  const tool = { id: targetDetails[target].tools[0], path: toolPath, sha256: sha('original compiler fixture'), version_label: 'fixture-only-1' };
  await writeFile(toolPath, 'original compiler fixture');
  const toolchain = join(scratch, 'tools.json');
  await writeFile(toolchain, JSON.stringify({ schema_version: 1, tools: [tool] }));
  return { scratch, bundle, inventory, tool, toolchain, config: { bundle, output: join(scratch, 'new-output'), toolchain, version: '0.4.0' } };
}

test('native arguments require explicit version, fresh absolute paths and known options', () => {
  const args = ['--bundle', resolve('bundle'), '--output', resolve('candidate'), '--toolchain', resolve('tools.json'), '--version', '1.2.3'];
  assert.equal(parseArguments(args).version, '1.2.3');
  for (const invalidArgs of [[...args, '--version', '2.0.0'], [...args, '--install'], [...args.slice(0, -1), '1.2.3\n%pre'], [...args.slice(0, -1), '01.2.3'], ['--bundle', 'relative', ...args.slice(2)]]) assert.throws(() => parseArguments(invalidArgs));
});

test('native host selection rejects cross-CPU, cross-OS and the wrong Linux distribution', () => {
  for (const [target, detail] of Object.entries(targetDetails)) {
    const host = { platform: detail.platform, arch: detail.arch, release: '23.0.0', osRelease: target.startsWith('rocky') ? 'ID="rocky"\nVERSION_ID="9.6"' : 'ID=ubuntu\nVERSION_ID="24.04"' };
    assert.doesNotThrow(() => assertHost(target, host));
    assert.throws(() => assertHost(target, { ...host, arch: 'wrong' }));
    assert.throws(() => assertHost(target, { ...host, platform: 'wrong' }));
    if (detail.platform === 'linux') assert.throws(() => assertHost(target, { ...host, osRelease: 'ID=ubuntu\nVERSION_ID=26.04' }));
  }
});

test('binary headers distinguish executable architecture and reject libraries and truncated data', () => {
  for (const target of Object.keys(targetDetails)) {
    const bytes = binary(target);
    assert(binaryMatches(bytes, target));
    assert(!binaryMatches(bytes.subarray(0, 20), target));
    assert(!binaryMatches(Buffer.alloc(256), target));
    if (target.startsWith('windows')) { bytes.writeUInt16LE(0x2002, 150); assert(!binaryMatches(bytes, target)); }
    if (target.endsWith('arm64')) assert(!binaryMatches(binary(target.replace('arm64', 'x86_64')), target));
  }
});

test('actual bundles verify for all six targets without claiming live target behavior', async (t) => {
  for (const target of Object.keys(targetDetails)) {
    const f = await fixture(t, target);
    assert.deepEqual(await verifyBundle(f.bundle), f.inventory);
  }
});

test('source tampering, GUI file workers, extra metadata and wrong headers fail before output', async (t) => {
  for (const change of ['binary', 'header', 'gui-worker', 'index', 'extra', 'development']) {
    const f = await fixture(t);
    if (change === 'binary') await writeFile(join(f.bundle, 'bin/forme-hostd.exe'), 'tampered bytes');
    if (change === 'header') {
      const entry = f.inventory.files.find((file) => file.path === 'bin/forme-hostd.exe');
      const value = binary('macos-arm64'); await writeFile(join(f.bundle, entry.path), value);
      entry.sha256 = sha(value); entry.bytes = value.length;
    }
    if (change === 'gui-worker') {
      const entry = f.inventory.files.find((file) => file.path === 'bin/forme-file-worker.exe');
      const value = fileWorkerPe(['user32.dll']); await writeFile(join(f.bundle, entry.path), value);
      entry.sha256 = sha(value); entry.bytes = value.length;
    }
    if (change === 'index') await writeFile(join(f.bundle, 'SHA256SUMS'), 'fake index');
    if (change === 'extra') await writeFile(join(f.bundle, 'unlisted'), 'extra');
    if (change === 'development') f.inventory.development = true;
    if (['header', 'gui-worker', 'development'].includes(change)) {
      f.inventory.content_digest = sha(JSON.stringify(f.inventory.files));
      await writeFile(join(f.bundle, 'bundle-inventory.json'), JSON.stringify(f.inventory));
      await writeFile(join(f.bundle, 'SHA256SUMS'), `${f.inventory.files.map((file) => `${file.sha256}  ${file.path}`).join('\n')}\n`);
    }
    await assert.rejects(buildNativePackage(f.config));
    await assert.rejects(readdir(f.config.output), { code: 'ENOENT' });
  }
});

test('compiler pins are closed, exact and checked again immediately before execution', async (t) => {
  const f = await fixture(t);
  const tools = await verifyToolchain(f.toolchain, ['iscc']);
  assert.equal(tools.get('iscc').sha256, f.tool.sha256);
  await writeFile(f.tool.path, 'substituted compiler');
  await assert.rejects(recheckTool(f.tool));
  await assert.rejects(verifyToolchain(f.toolchain, ['iscc']));
  await writeFile(f.tool.path, 'original compiler fixture');
  await writeFile(f.toolchain, JSON.stringify({ schema_version: 1, tools: [f.tool, f.tool] }));
  await assert.rejects(verifyToolchain(f.toolchain, ['iscc']));
});

test('hardlinked bundle bytes cannot be packaged through an alias', async (t) => {
  const f = await fixture(t), original = join(f.bundle, 'NOTICE');
  await link(original, join(f.scratch, 'outside-hardlink'));
  await assert.rejects(verifyBundle(f.bundle));
});

test('native payloads carry actual hooks, service files, Web assets and licenses', async (t) => {
  for (const target of ['ubuntu2404-arm64', 'rocky9-x86_64', 'macos-arm64']) {
    const f = await fixture(t, target), output = join(f.scratch, 'layout');
    await mkdir(output);
    const paths = { bundle: f.bundle, work: join(output, 'work'), artifacts: join(output, 'artifacts'), payload: join(output, 'work/payload'), packageName: 'forme-fixture' };
    await prepareLayout(targetDetails[target].format, paths, f.inventory, '0.4.0');
    const program = target.startsWith('macos') ? 'Library/ForMe' : 'opt/forme';
    for (const name of ['LICENSE', 'NOTICE', 'packaging/posix/service-access.sh', 'share/forme/web/index.html']) assert.deepEqual(await readFile(join(paths.payload, program, name)), await readFile(join(f.bundle, name)));
    const commands = nativeCommands(targetDetails[target].format, paths, '0.4.0');
    assert(commands.length > 0 && commands.every((command) => command.args.every((arg) => typeof arg === 'string')));
    if (target.startsWith('ubuntu')) {
      const control = await readFile(join(paths.payload, 'DEBIAN/control'), 'utf8');
      assert.match(control, /^Version: 0\.4\.0$/m); assert.match(control, /^Architecture: arm64$/m);
      for (const hook of ['preinst', 'postinst', 'prerm']) assert(!(await readFile(join(paths.payload, 'DEBIAN', hook), 'utf8')).includes('\r'));
    } else if (target.startsWith('rocky')) assert.match(await readFile(join(paths.work, 'SPECS/forme.spec'), 'utf8'), /%install\nmkdir -p/);
    else {
      assert.match(await readFile(join(paths.work, 'Distribution.xml'), 'utf8'), /version="0.4.0"/);
      assert.match(await readFile(join(paths.work, 'scripts/preinstall'), 'utf8'), /\[ "\$\(uname -m\)" = arm64 \] \|\| exit 1/);
      assert((await readFile(join(paths.payload, 'Library/LaunchDaemons/com.forme.brain.plist'), 'utf8')).includes('com.forme.brain'));
    }
  }
});

test('Windows compiler reads an independent verified snapshot and leaves the source unchanged', async (t) => {
  const f = await fixture(t), output = join(f.scratch, 'layout');
  await mkdir(output);
  const paths = { bundle: f.bundle, compilerBundle: join(output, 'work/compiler-inputs'), work: join(output, 'work'), artifacts: join(output, 'artifacts'), packageName: 'forme-fixture' };
  const snapshot = await prepareLayout('exe', paths, f.inventory, '0.4.0');
  assert.equal(snapshot, paths.compilerBundle);
  assert.deepEqual(await verifyBundle(snapshot), f.inventory);
  const [command] = nativeCommands('exe', paths, '0.4.0');
  assert(command.args.includes(`/DReleaseRoot=${snapshot}`));
  assert.equal(command.args.at(-1), join(snapshot, 'packaging/windows/forme.iss'));
  await writeFile(join(snapshot, 'NOTICE'), 'compiler-mutated input');
  await assert.rejects(verifyBundle(snapshot));
  assert.deepEqual(await verifyBundle(f.bundle), f.inventory);
});
