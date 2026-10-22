#!/usr/bin/env node
// Offline release packaging. Private keys never enter a bundle, log or receipt.
import { createHash, createPrivateKey, createPublicKey, sign } from 'node:crypto';
import { createReadStream } from 'node:fs';
import { copyFile, lstat, mkdir, readdir, readFile, writeFile } from 'node:fs/promises';
import { constants } from 'node:fs';
import { dirname, isAbsolute, join, relative, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';

const targets = Object.freeze({
  'windows-x86_64': 'windows_x86_64', 'macos-arm64': 'macos_arm64',
  'macos-x86_64': 'macos_x86_64', 'ubuntu2404-x86_64': 'ubuntu2404_x86_64',
  'ubuntu2404-arm64': 'ubuntu2404_arm64', 'rocky9-x86_64': 'rocky9_x86_64',
});
const sha = (bytes) => `sha256:${createHash('sha256').update(bytes).digest('hex')}`;
const digest = (value) => sha(Buffer.from(JSON.stringify(value)));
const within = (parent, child) => {
  const path = relative(parent, child);
  return path === '' || (!path.startsWith(`..${sep}`) && path !== '..' && !isAbsolute(path));
};
const ordered = (a, b) => a < b ? -1 : a > b ? 1 : 0;
const invalid = () => new Error('Invalid signed-release input; existing files were retained');
const closed = (object, fields) => {
  if (!object || Array.isArray(object) || typeof object !== 'object' ||
      Object.keys(object).some((key) => !fields.includes(key)) || fields.some((key) => !Object.hasOwn(object, key))) throw invalid();
};
const safeName = (name) => typeof name === 'string' && /^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$/.test(name) && !name.endsWith('.');

async function checked(path, missing = false) {
  if (!isAbsolute(path) || resolve(path) !== path) throw invalid();
  let current = path;
  while (true) {
    try {
      const stat = await lstat(current);
      if (stat.isSymbolicLink()) throw invalid();
    } catch (error) {
      if (error.code !== 'ENOENT' || !missing) throw invalid();
    }
    const parent = dirname(current);
    if (parent === current) return;
    current = parent;
  }
}

async function bounded(path, limit) {
  await checked(path);
  const stat = await lstat(path);
  if (!stat.isFile() || stat.size > limit) throw invalid();
  const bytes = await readFile(path);
  if (bytes.length !== stat.size) throw invalid();
  return bytes;
}

async function fileDigest(path, size) {
  await checked(path);
  const stat = await lstat(path);
  if (!stat.isFile() || stat.size !== size || size > 2 * 1024 ** 3) throw invalid();
  const hash = createHash('sha256');
  let count = 0;
  for await (const chunk of createReadStream(path)) {
    count += chunk.length;
    if (count > size) throw invalid();
    hash.update(chunk);
  }
  if (count !== size) throw invalid();
  return `sha256:${hash.digest('hex')}`;
}

function safeRelative(name) {
  if (typeof name !== 'string' || name.length > 512 || !/^[\x20-\x7e]+$/.test(name) ||
      /[\\:\x00-\x1f<>"|?*]/.test(name) || name.split('/').some((part) =>
        !part || part === '.' || part === '..' || /[ .]$/.test(part) ||
        /^(con|prn|aux|nul|com[1-9]|lpt[1-9])(?:\.|$)/i.test(part))) throw invalid();
  return name;
}

async function inventoryFiles(root, current = root, depth = 0, result = []) {
  if (depth > 16 || result.length > 10_000) throw invalid();
  await checked(current);
  for (const entry of await readdir(current, { withFileTypes: true })) {
    const path = join(current, entry.name);
    if (entry.isDirectory()) await inventoryFiles(root, path, depth + 1, result);
    else if (entry.isFile()) result.push(relative(root, path).split(sep).join('/'));
    else throw invalid();
  }
  if (result.length > 10_000) throw invalid();
  return result;
}

function range(value) {
  closed(value, ['schema_version', 'read_min', 'read_max', 'write_version']);
  if (value.schema_version !== 1 || [value.read_min, value.read_max, value.write_version].some((v) =>
    !Number.isSafeInteger(v) || v < 1 || v > 0xffffffff) ||
    value.read_min > value.write_version || value.write_version > value.read_max) throw invalid();
  // Field order is the frozen Rust protocol's serde order, not alphabetic JSON.
  return { schema_version: 1, read_min: value.read_min, read_max: value.read_max, write_version: value.write_version };
}

function parse(args) {
  const values = {};
  const names = { '--bundle': 'bundle', '--output': 'output', '--key-file': 'keyFile',
    '--key-id': 'keyId', '--compatibility-file': 'compatibilityFile' };
  for (let index = 0; index < args.length; index += 2) {
    const key = names[args[index]], value = args[index + 1];
    if (!key || Object.hasOwn(values, key) || !value || value.startsWith('--')) throw invalid();
    values[key] = value;
  }
  closed(values, Object.values(names));
  return values;
}

export async function signBundle(config) {
  closed(config, ['bundle', 'output', 'keyFile', 'keyId', 'compatibilityFile']);
  const { bundle, output, keyFile, keyId, compatibilityFile } = config;
  if (![bundle, output, keyFile, compatibilityFile].every((path) => typeof path === 'string' && isAbsolute(path) && resolve(path) === path) ||
      !safeName(keyId) || within(bundle, output) || within(output, bundle) ||
      within(bundle, keyFile) || within(output, keyFile) || within(output, compatibilityFile)) throw invalid();
  await checked(output, true);
  try { await lstat(output); throw invalid(); } catch (error) { if (error.code !== 'ENOENT') throw invalid(); }
  const inventory = JSON.parse(await bounded(join(bundle, 'bundle-inventory.json'), 8 * 1024 ** 2));
  if (inventory.schema_version !== 1 || !Object.hasOwn(targets, inventory.target) ||
      !safeName(inventory.release_id) || !Array.isArray(inventory.files) || !inventory.files.length || inventory.files.length > 10_000) throw invalid();
  const compatibility = JSON.parse(await bounded(compatibilityFile, 4096));
  closed(compatibility, ['schema_version', 'config_range', 'data_range', 'minimum_upgrader']);
  if (compatibility.schema_version !== 1 || !Number.isSafeInteger(compatibility.minimum_upgrader) ||
      compatibility.minimum_upgrader < 1 || compatibility.minimum_upgrader > 0xffffffff) throw invalid();
  const configRange = range(compatibility.config_range), dataRange = range(compatibility.data_range);
  const keyBytes = await bounded(keyFile, 16 * 1024);
  if (process.platform !== 'win32' && ((await lstat(keyFile)).mode & 0o077) !== 0) throw invalid();
  let privateKey;
  try { privateKey = createPrivateKey(keyBytes); } finally { keyBytes.fill(0); }
  if (privateKey.asymmetricKeyType !== 'ed25519') throw invalid();
  const publicDer = createPublicKey(privateKey).export({ type: 'spki', format: 'der' });
  const publicBytes = Array.from(publicDer.subarray(-32));
  const actual = (await inventoryFiles(bundle)).sort(ordered);
  const names = inventory.files.map((file) => safeRelative(file.path));
  if (new Set(names.map((name) => name.toLowerCase())).size !== names.length ||
      JSON.stringify(actual) !== JSON.stringify([...names, 'bundle-inventory.json'].sort(ordered))) throw invalid();
  const mapped = [], collision = new Set();
  let total = 0;
  for (const file of inventory.files) {
    if (!Number.isSafeInteger(file.bytes) || file.bytes < 0 || !/^[0-9a-f]{64}$/.test(file.sha256)) throw invalid();
    total += file.bytes;
    if (total > 16 * 1024 ** 3) throw invalid();
    const name = file.path.startsWith('share/forme/web/') ? `web/${file.path.slice('share/forme/web/'.length)}` : file.path;
    safeRelative(name);
    if (collision.has(name.toLowerCase())) throw invalid();
    collision.add(name.toLowerCase());
    const hash = `sha256:${file.sha256}`;
    if (await fileDigest(join(bundle, file.path), file.bytes) !== hash) throw invalid();
    mapped.push({ source: file.path, schema_version: 1, relative_path: name, digest: hash, size: file.bytes });
  }
  mapped.sort((a, b) => ordered(a.relative_path, b.relative_path));
  const files = mapped.map(({ schema_version, relative_path, digest: hash, size }) => ({ schema_version, relative_path, digest: hash, size }));
  const suffix = inventory.target === 'windows-x86_64' ? '.exe' : '';
  for (const binary of ['forme-hostd', 'forme-webd', 'forme-gatewayd', 'forme-cli', 'forme-hostctl']) {
    if (!collision.has(`bin/${binary}${suffix}`)) throw invalid();
  }
  const ui = files.filter((file) => file.relative_path.startsWith('web/'));
  if (!ui.length) throw invalid();
  const manifest = { schema_version: 1, release_id: inventory.release_id, target: targets[inventory.target], files,
    ui_digest: digest(ui), managed_dependencies: [...new Set(files.filter((file) => file.relative_path === 'web/dependencies.json').map((file) => file.digest))].sort(ordered), config_range: configRange, data_range: dataRange,
    minimum_upgrader: compatibility.minimum_upgrader, signature_ref: keyId };
  const manifestDigest = digest(manifest);
  const signature = sign(null, Buffer.from(manifestDigest), privateKey);
  await mkdir(output); // New only: an interrupted output is retained for explicit review.
  await mkdir(join(output, 'content'));
  for (const file of mapped) {
    const destination = join(output, 'content', file.relative_path);
    await mkdir(dirname(destination), { recursive: true });
    await checked(join(bundle, file.source));
    await checked(destination, true);
    await copyFile(join(bundle, file.source), destination, constants.COPYFILE_EXCL);
    if (await fileDigest(destination, file.size) !== file.digest) throw invalid();
  }
  await writeFile(join(output, 'manifest.json'), JSON.stringify(manifest), { flag: 'wx', mode: 0o644 });
  await writeFile(join(output, 'signature.ed25519'), signature, { flag: 'wx', mode: 0o644 });
  const receipt = { schema_version: 1, release_id: manifest.release_id, target: manifest.target,
    release_digest: manifestDigest, key_id: keyId, public_key: publicBytes,
    native_package_signature: 'not_performed', native_installation: 'not_performed' };
  await writeFile(join(output, 'signing-receipt.json'), JSON.stringify(receipt), { flag: 'wx', mode: 0o644 });
  return receipt;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try { console.log(JSON.stringify(await signBundle(parse(process.argv.slice(2))))); }
  catch { console.error('Signed release generation failed; existing inputs and interrupted output were retained'); process.exitCode = 1; }
}
