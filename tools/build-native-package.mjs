#!/usr/bin/env node
// Compile an unsigned native candidate. Never installs or publishes anything.
import { spawn } from 'node:child_process';
import { createHash } from 'node:crypto';
import { copyFile, lstat, mkdir, open, readdir, readFile, writeFile } from 'node:fs/promises';
import { constants } from 'node:fs';
import { arch, release } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { checked, fileDigest, invalid, recheckTool, sha, targetDetails, verifyBundle, verifyToolchain, within } from './native-package/inputs.mjs';
import { nativeCommands, prepareLayout } from './native-package/layout.mjs';

export function parseArguments(args) {
  const config = { allowDevelopment: false };
  const options = { '--bundle': 'bundle', '--output': 'output', '--toolchain': 'toolchain', '--version': 'version' };
  for (let index = 0; index < args.length; index += 1) {
    if (args[index] === '--allow-development' && !config.allowDevelopment) { config.allowDevelopment = true; continue; }
    const key = options[args[index]], value = args[++index];
    if (!key || Object.hasOwn(config, key) || !value || value.startsWith('--')) throw invalid();
    config[key] = value;
  }
  if (!['bundle', 'output', 'toolchain', 'version'].every((key) => Object.hasOwn(config, key)) ||
      !/^(0|[1-9][0-9]{0,4})\.(0|[1-9][0-9]{0,4})\.(0|[1-9][0-9]{0,4})$/.test(config.version)) throw invalid();
  for (const key of ['bundle', 'output', 'toolchain']) {
    if (typeof config[key] !== 'string' || !/^(?:[A-Za-z]:[\\/]|\/)/.test(config[key])) throw invalid();
    config[key] = resolve(config[key]);
  }
  return config;
}

export function assertHost(target, host = { platform: process.platform, arch: arch(), release: release(), osRelease: '' }) {
  const detail = targetDetails[target];
  if (!detail || detail.platform !== host.platform || detail.arch !== host.arch) throw new Error('Native packaging requires its exact OS and CPU; cross-builds are not accepted');
  if (detail.platform === 'darwin' && Number(host.release.split('.')[0]) < 23) throw invalid();
  if (detail.platform === 'linux') {
    const values = Object.fromEntries(host.osRelease.split(/\r?\n/).filter((line) => /^[A-Z_]+=.*/.test(line)).map((line) => {
      const index = line.indexOf('='); return [line.slice(0, index), line.slice(index + 1).replace(/^"(.*)"$/, '$1')];
    }));
    if (target.startsWith('ubuntu') ? values.ID !== 'ubuntu' || values.VERSION_ID !== '24.04' : values.ID !== 'rocky' || values.VERSION_ID.split('.')[0] !== '9') throw invalid();
  }
}

async function invoke(tool, args) {
  await recheckTool(tool);
  const env = {};
  for (const key of ['SystemRoot', 'WINDIR', 'TEMP', 'TMP', 'HOME', 'USERPROFILE']) if (process.env[key]) env[key] = process.env[key];
  env.PATH = process.platform === 'win32' ? `${dirname(tool.path)};${process.env.SystemRoot}\\System32` : '/usr/sbin:/usr/bin:/sbin:/bin';
  env.LANG = 'C'; env.LC_ALL = 'C';
  const child = spawn(tool.path, args, { env, stdio: ['ignore', 'pipe', 'pipe'], windowsHide: true, shell: false });
  const hash = createHash('sha256');
  let count = 0, exceeded = false;
  for (const stream of [child.stdout, child.stderr]) stream.on('data', (bytes) => {
    count += bytes.length;
    if (count > 4 * 1024 ** 2) { exceeded = true; child.kill(); }
    else hash.update(bytes);
  });
  let expired = false;
  const timer = setTimeout(() => { expired = true; child.kill(); }, 15 * 60 * 1000);
  try {
    const code = await new Promise((resolveCode, reject) => {
      child.once('error', reject);
      child.once('close', resolveCode);
    });
    if (code !== 0 || expired || exceeded) throw new Error('Pinned package compiler did not complete; preserve and inspect this candidate directory');
    return { id: tool.id, sha256: tool.sha256, version_label: tool.version_label, command_digest: sha(JSON.stringify(args)), output_digest: hash.digest('hex'), exit_code: code };
  } finally { clearTimeout(timer); }
}

export async function buildNativePackage(config) {
  // Reuse parsing validation even for callers of the module API.
  config = parseArguments(['--bundle', config.bundle, '--output', config.output, '--toolchain', config.toolchain, '--version', config.version, ...(config.allowDevelopment ? ['--allow-development'] : [])]);
  await checked(config.bundle); await checked(config.output, true); await checked(config.toolchain);
  if (within(config.bundle, config.output) || within(config.output, config.bundle) || within(config.output, config.toolchain)) throw invalid();
  try { await lstat(config.output); throw invalid(); } catch (error) { if (error.code !== 'ENOENT') throw error; }
  const inventory = await verifyBundle(config.bundle, config.allowDevelopment);
  assertHost(inventory.target, { platform: process.platform, arch: arch(), release: release(), osRelease: process.platform === 'linux' ? await readFile('/etc/os-release', 'utf8') : '' });
  const detail = targetDetails[inventory.target];
  const tools = await verifyToolchain(config.toolchain, detail.tools);
  for (const tool of tools.values()) if (within(config.output, tool.path) || within(config.bundle, tool.path)) throw invalid();
  const packageName = `forme-${config.version}-${inventory.target}`;
  const paths = { bundle: config.bundle, compilerBundle: join(config.output, 'work/compiler-inputs'), work: join(config.output, 'work'), artifacts: join(config.output, 'artifacts'), payload: join(config.output, 'work/payload'), packageName };
  await mkdir(config.output, { mode: 0o700 });
  const packagedBundle = await prepareLayout(detail.format, paths, inventory, config.version);
  if (JSON.stringify(await verifyBundle(packagedBundle, config.allowDevelopment)) !== JSON.stringify(inventory)) throw invalid();
  const receipts = [];
  for (const command of nativeCommands(detail.format, paths, config.version)) receipts.push(await invoke(tools.get(command.id), command.args));
  if (detail.format === 'rpm') {
    const names = await readdir(join(paths.work, 'RPMS/x86_64'));
    if (names.length !== 1 || !names[0].endsWith('.rpm')) throw invalid();
    await copyFile(join(paths.work, 'RPMS/x86_64', names[0]), join(paths.artifacts, `${packageName}.rpm`), constants.COPYFILE_EXCL);
  }
  const artifact = join(paths.artifacts, `${packageName}.${detail.format}`);
  const stat = await lstat(artifact);
  if (!stat.isFile() || stat.size === 0) throw invalid();
  const file = await open(artifact, 'r');
  try {
    const header = Buffer.alloc(8);
    const { bytesRead } = await file.read(header, 0, header.length, 0);
    const valid = detail.format === 'exe' ? header.toString('ascii', 0, 2) === 'MZ' :
      detail.format === 'deb' ? header.toString('ascii') === '!<arch>\n' :
      detail.format === 'rpm' ? header.subarray(0, 4).equals(Buffer.from('edabeedb', 'hex')) : header.toString('ascii', 0, 4) === 'xar!';
    if (bytesRead !== 8 || !valid) throw invalid();
  } finally { await file.close(); }
  const artifactDigest = await fileDigest(artifact, stat.size);
  // Check source again: package compilation must not mutate reviewed inputs.
  const after = await verifyBundle(config.bundle, config.allowDevelopment);
  if (JSON.stringify(after) !== JSON.stringify(inventory) || JSON.stringify(await verifyBundle(packagedBundle, config.allowDevelopment)) !== JSON.stringify(inventory)) throw invalid();
  const receipt = {
    schema_version: 1, release_id: inventory.release_id, target: inventory.target, version: config.version,
    source_bundle_digest: inventory.content_digest, binary_target_verification: 'header_only', development: inventory.development,
    artifact: { path: `${packageName}.${detail.format}`, bytes: stat.size, sha256: artifactDigest }, tools: receipts,
    native_compilation: 'performed', native_package_signature: 'not_performed', native_installation: 'not_performed', release_ready: false,
  };
  await writeFile(join(config.output, 'native-build-receipt.json'), `${JSON.stringify(receipt, null, 2)}\n`, { flag: 'wx', mode: 0o600 });
  return receipt;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const receipt = await buildNativePackage(parseArguments(process.argv.slice(2)));
    console.log(`Native ${receipt.target} candidate compiled; signing and installation: NOT_RUN`);
  } catch (error) { console.error(`native package: ${error.message}`); process.exitCode = 1; }
}
