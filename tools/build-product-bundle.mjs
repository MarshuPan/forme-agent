#!/usr/bin/env node
// Package reviewed inputs only. Never install, delete an existing output,
// read owner runtime data, or treat this inventory as a signed ReleaseManifest.
import { createHash } from 'node:crypto';
import { lstat, mkdir, readdir, readFile, writeFile } from 'node:fs/promises';
import { dirname, isAbsolute, join, relative, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';
import { productBinaries } from './product-binaries.mjs';

const repository = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const requiredPackagingFiles = Object.freeze({
  windows: ['forme.iss', 'install-access.ps1', 'service-access.psm1'],
  linux: ['forme-brain.service', 'forme-host.service', 'forme-web.service', 'forme.control', 'forme.preinst', 'forme.postinst', 'forme.prerm', 'forme.spec'],
  macos: ['Distribution.xml', 'com.forme.brain.plist', 'com.forme.host.plist', 'com.forme.web.plist', 'scripts/preinstall', 'scripts/postinstall'],
});
export const targets = Object.freeze({
  'windows-x86_64': { extension: '.exe', template: 'windows' },
  'macos-arm64': { extension: '', template: 'macos' },
  'macos-x86_64': { extension: '', template: 'macos' },
  'ubuntu2404-x86_64': { extension: '', template: 'linux' },
  'ubuntu2404-arm64': { extension: '', template: 'linux' },
  'rocky9-x86_64': { extension: '', template: 'linux' },
});
const sha = (bytes) => createHash('sha256').update(bytes).digest('hex');
const slash = (value) => value.split(sep).join('/');
const within = (parent, child) => {
  const path = relative(parent, child);
  return path === '' || (!path.startsWith(`..${sep}`) && path !== '..' && !isAbsolute(path));
};

export function parseArguments(args) {
  const values = new Map();
  const allowed = new Set(['target', 'release-id', 'binary-dir', 'output', 'development']);
  for (let index = 0; index < args.length; index += 1) {
    const name = args[index].slice(2);
    if (!args[index].startsWith('--') || !allowed.has(name) || values.has(name)) throw new Error('Invalid or duplicate bundle argument');
    if (name === 'development') { values.set(name, true); continue; }
    const value = args[++index];
    if (!value || value.startsWith('--')) throw new Error('Missing bundle argument');
    values.set(name, value);
  }
  const config = { target: values.get('target'), releaseId: values.get('release-id'), binaryDir: values.get('binary-dir'), output: values.get('output'), development: values.get('development') === true };
  validateArguments(config);
  return config;
}

function validateArguments(config) {
  if (!Object.hasOwn(targets, config.target) || typeof config.releaseId !== 'string' ||
      !/^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$/.test(config.releaseId) ||
      typeof config.output !== 'string' || !isAbsolute(config.output) ||
      typeof config.binaryDir !== 'string' || !isAbsolute(config.binaryDir)) throw new Error('Bundle requires a known target, release id and absolute input/output paths');
  if (typeof config.development !== 'boolean') throw new Error('Bundle mode must be explicit');
  if (!config.development && /(?:^|[\\/])debug(?:[\\/]|$)/i.test(config.binaryDir)) throw new Error('Debug binaries require --development');
}

async function checkedPath(path, missingLeaf = false) {
  let current = resolve(path);
  let first = true;
  while (true) {
    try {
      const info = await lstat(current);
      if (info.isSymbolicLink()) throw new Error('Linked paths are not bundle inputs or outputs');
    } catch (error) {
      if (error.code !== 'ENOENT' || !(missingLeaf && first)) throw error;
    }
    const parent = dirname(current);
    if (parent === current) break;
    current = parent;
    first = false;
  }
}

async function filesIn(directory) {
  await checkedPath(directory);
  const output = [];
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    const path = join(directory, entry.name);
    if (entry.isDirectory()) output.push(...await filesIn(path));
    else if (entry.isFile()) output.push(path);
    else throw new Error('Unsupported file type in bundle input');
  }
  return output.sort();
}

async function regularFile(path) {
  await checkedPath(path);
  const info = await lstat(path);
  if (!info.isFile() || info.size > 512 * 1024 * 1024) throw new Error('Bundle input is not a bounded regular file');
  const bytes = await readFile(path);
  if (bytes.length !== info.size) throw new Error('Bundle input changed during read');
  return { bytes, mode: info.mode & 0o777 };
}

async function webInputs(root) {
  const directory = join(root, 'crates/gateway/assets/product/dist');
  const { bytes } = await regularFile(join(directory, 'manifest.json'));
  const inventory = JSON.parse(bytes);
  if (inventory.schema_version !== 1 || !Array.isArray(inventory.files) || inventory.files.length > 64) throw new Error('Invalid Web asset inventory');
  const names = new Set(['manifest.json']);
  for (const file of inventory.files) {
    if (!/^(index\.html|favicon\.svg|LICENSES\.txt|dependencies\.json|assets\/[A-Za-z0-9_-]+\.(js|css))$/.test(file.path) || names.has(file.path)) throw new Error('Unexpected or duplicate Web asset');
    names.add(file.path);
    const input = await regularFile(join(directory, file.path));
    if (sha(input.bytes) !== file.sha256 || input.bytes.length !== file.bytes) throw new Error('Web asset differs from its reviewed inventory');
  }
  for (const required of ['index.html', 'LICENSES.txt', 'dependencies.json']) if (!names.has(required)) throw new Error('Missing required Web license or asset');
  const actual = await filesIn(directory);
  if (actual.length !== names.size || actual.some((path) => !names.has(slash(relative(directory, path))))) throw new Error('Undeclared Web asset');
  return actual.map((source) => ({ source, path: `share/forme/web/${slash(relative(directory, source))}` }));
}

export async function buildBundle(config, root = repository) {
  validateArguments(config);
  root = resolve(root);
  const output = resolve(config.output);
  const binaryDir = resolve(config.binaryDir);
  // No recursive cleanup is necessary: the output must be new, with an existing
  // real parent. Inside the repo only target/product-bundles is permitted.
  if (within(output, root) || within(output, binaryDir) || within(binaryDir, output) ||
      (within(root, output) && (!within(join(root, 'target/product-bundles'), output) || output === join(root, 'target/product-bundles')))) throw new Error('Unsafe or overlapping bundle output');
  await checkedPath(output, true);
  await checkedPath(binaryDir);
  const target = targets[config.target];
  const inputs = productBinaries.map((name) => ({ source: join(binaryDir, name + target.extension), path: `bin/${name}${target.extension}` }));
  inputs.push(...await webInputs(root));
  for (const name of ['LICENSE', 'NOTICE']) inputs.push({ source: join(root, name), path: name });
  const templates = join(root, 'packaging', target.template);
  const templateFiles = await filesIn(templates);
  const templateRelativePaths = new Set(templateFiles.map((source) => slash(relative(templates, source))));
  for (const required of requiredPackagingFiles[target.template]) {
    if (!templateRelativePaths.has(required)) throw new Error(`Packaging template is missing required input: ${required}`);
  }
  for (const source of templateFiles) inputs.push({ source, path: `packaging/${slash(relative(join(root, 'packaging'), source))}` });
  if (target.template !== 'windows') {
    inputs.push({ source: join(root, 'packaging/posix/service-access.sh'), path: 'packaging/posix/service-access.sh' });
  }
  // Verify every input before creating any output, including executable bytes.
  for (const input of inputs) {
    const { bytes } = await regularFile(input.source);
    input.digest = sha(bytes);
  }
  inputs.sort((left, right) => left.path < right.path ? -1 : left.path > right.path ? 1 : 0);
  await mkdir(output); // EEXIST, including an empty directory, is a hard refusal.
  const files = [];
  for (const input of inputs) {
    const { bytes, mode } = await regularFile(input.source);
    if (sha(bytes) !== input.digest) throw new Error('Bundle input changed after preflight; preserve this incomplete output for inspection');
    const destination = join(output, input.path);
    await mkdir(dirname(destination), { recursive: true });
    await writeFile(destination, bytes, { flag: 'wx', mode });
    files.push({ path: input.path, bytes: bytes.length, sha256: sha(bytes) });
  }
  const inventory = {
    schema_version: 1, product: 'forme', release_id: config.releaseId,
    target: config.target, development: config.development, files,
    content_digest: sha(JSON.stringify(files)),
    signature: 'not_signed', native_installation: 'not_performed', release_ready: false,
    binary_target_verification: 'not_performed',
  };
  await writeFile(join(output, 'bundle-inventory.json'), `${JSON.stringify(inventory, null, 2)}\n`, { flag: 'wx' });
  await writeFile(join(output, 'SHA256SUMS'), `${files.map((file) => `${file.sha256}  ${file.path}`).join('\n')}\n`, { flag: 'wx' });
  return inventory;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const inventory = await buildBundle(parseArguments(process.argv.slice(2)));
    console.log(`Product bundle: ${inventory.target}; ${inventory.files.length} files; unsigned; native installation: NOT_RUN`);
  } catch (error) {
    console.error(`product bundle: ${error.message}`);
    process.exitCode = 1;
  }
}
