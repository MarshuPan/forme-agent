// Original closed bundle validation for native package compilation.
import { createHash } from 'node:crypto';
import { createReadStream } from 'node:fs';
import { lstat, open, readdir, readFile } from 'node:fs/promises';
import { dirname, isAbsolute, join, relative, resolve, sep } from 'node:path';

export const invalid = () => new Error('Native package input is invalid or changed; existing files are retained');
export const sha = (bytes) => createHash('sha256').update(bytes).digest('hex');
export const within = (parent, path) => {
  const name = relative(parent, path);
  return name === '' || (name !== '..' && !name.startsWith(`..${sep}`) && !isAbsolute(name));
};
export const closed = (value, keys) => {
  if (!value || typeof value !== 'object' || Array.isArray(value) ||
      Object.keys(value).some((key) => !keys.includes(key)) || keys.some((key) => !Object.hasOwn(value, key))) throw invalid();
};

export async function checked(path, missing = false) {
  if (typeof path !== 'string' || !isAbsolute(path) || resolve(path) !== path || /[\x00-\x1f%"'`$]/.test(path)) throw invalid();
  for (let current = path;; current = dirname(current)) {
    try { if ((await lstat(current)).isSymbolicLink()) throw invalid(); }
    catch (error) { if (error.code !== 'ENOENT' || !missing) throw error; }
    if (dirname(current) === current) return;
  }
}

export async function bounded(path, limit) {
  await checked(path);
  const stat = await lstat(path);
  if (!stat.isFile() || stat.nlink !== 1 || stat.size > limit) throw invalid();
  const bytes = await readFile(path);
  if (bytes.length !== stat.size) throw invalid();
  return bytes;
}

export async function fileDigest(path, size) {
  await checked(path);
  const stat = await lstat(path);
  if (!stat.isFile() || stat.nlink !== 1 || stat.size !== size || size > 2 * 1024 ** 3) throw invalid();
  const hash = createHash('sha256');
  let count = 0;
  for await (const chunk of createReadStream(path)) {
    count += chunk.length;
    if (count > size) throw invalid();
    hash.update(chunk);
  }
  if (count !== size) throw invalid();
  return hash.digest('hex');
}

function directRelative(value) {
  if (typeof value !== 'string' || value.length > 512 || !/^[A-Za-z0-9_.\-/]+$/.test(value) ||
      value.split('/').some((part) => !part || part === '.' || part === '..' || part.endsWith('.') ||
        /^(con|prn|aux|nul|com[1-9]|lpt[1-9])(?:\.|$)/i.test(part))) throw invalid();
  return value;
}

async function list(root, directory = root, depth = 0, files = []) {
  if (depth > 16 || files.length > 10_000) throw invalid();
  await checked(directory);
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    const path = join(directory, entry.name);
    if (entry.isDirectory()) await list(root, path, depth + 1, files);
    else if (entry.isFile()) files.push(relative(root, path).split(sep).join('/'));
    else throw invalid();
  }
  if (files.length > 10_000) throw invalid();
  return files;
}

export const targetDetails = Object.freeze({
  'windows-x86_64': { platform: 'win32', arch: 'x64', format: 'exe', tools: ['iscc'] },
  'ubuntu2404-x86_64': { platform: 'linux', arch: 'x64', format: 'deb', tools: ['dpkg-deb'] },
  'ubuntu2404-arm64': { platform: 'linux', arch: 'arm64', format: 'deb', tools: ['dpkg-deb'] },
  'rocky9-x86_64': { platform: 'linux', arch: 'x64', format: 'rpm', tools: ['rpmbuild'] },
  'macos-x86_64': { platform: 'darwin', arch: 'x64', format: 'pkg', tools: ['pkgbuild', 'productbuild'] },
  'macos-arm64': { platform: 'darwin', arch: 'arm64', format: 'pkg', tools: ['pkgbuild', 'productbuild'] },
});

export function binaryMatches(bytes, target) {
  const detail = targetDetails[target];
  if (!detail || bytes.length < 64) return false;
  if (detail.platform === 'win32') {
    if (bytes.toString('ascii', 0, 2) !== 'MZ') return false;
    const offset = bytes.readUInt32LE(60);
    return offset >= 64 && offset + 26 <= bytes.length && bytes.readUInt32LE(offset) === 0x4550 &&
      bytes.readUInt16LE(offset + 4) === 0x8664 && bytes.readUInt16LE(offset + 24) === 0x20b &&
      (bytes.readUInt16LE(offset + 22) & 0x2002) === 2;
  }
  if (detail.platform === 'linux') {
    return bytes.toString('hex', 0, 4) === '7f454c46' && bytes[4] === 2 && bytes[5] === 1 && bytes[6] === 1 &&
      [2, 3].includes(bytes.readUInt16LE(16)) && bytes.readUInt16LE(18) === (detail.arch === 'arm64' ? 183 : 62) && bytes.readUInt32LE(20) === 1;
  }
  return bytes.readUInt32LE(0) === 0xfeedfacf && bytes.readUInt32LE(4) === (detail.arch === 'arm64' ? 0x100000c : 0x1000007) && bytes.readUInt32LE(12) === 2;
}

export async function verifyBundle(root, allowDevelopment = false) {
  const inventory = JSON.parse(await bounded(join(root, 'bundle-inventory.json'), 8 * 1024 ** 2));
  closed(inventory, ['schema_version', 'product', 'release_id', 'target', 'development', 'files', 'content_digest', 'signature', 'native_installation', 'release_ready', 'binary_target_verification']);
  if (inventory.schema_version !== 1 || inventory.product !== 'forme' || !Object.hasOwn(targetDetails, inventory.target) ||
      !/^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$/.test(inventory.release_id) ||
      typeof inventory.development !== 'boolean' || (inventory.development && !allowDevelopment) ||
      inventory.release_ready !== false || inventory.signature !== 'not_signed' || inventory.native_installation !== 'not_performed' ||
      inventory.binary_target_verification !== 'not_performed' || !Array.isArray(inventory.files) ||
      inventory.files.length === 0 || inventory.files.length > 10_000 || sha(JSON.stringify(inventory.files)) !== inventory.content_digest) throw invalid();
  let total = 0;
  const names = [];
  for (const file of inventory.files) {
    closed(file, ['path', 'bytes', 'sha256']);
    names.push(directRelative(file.path));
    if (!Number.isSafeInteger(file.bytes) || file.bytes < 0 || !/^[0-9a-f]{64}$/.test(file.sha256)) throw invalid();
    total += file.bytes;
    if (total > 16 * 1024 ** 3 || await fileDigest(join(root, file.path), file.bytes) !== file.sha256) throw invalid();
  }
  const expected = [...names, 'bundle-inventory.json', 'SHA256SUMS'].sort();
  if (new Set(expected.map((path) => path.toLowerCase())).size !== expected.length ||
      JSON.stringify((await list(root)).sort()) !== JSON.stringify(expected)) throw invalid();
  const sums = `${inventory.files.map((file) => `${file.sha256}  ${file.path}`).join('\n')}\n`;
  if ((await bounded(join(root, 'SHA256SUMS'), 8 * 1024 ** 2)).toString('utf8') !== sums) throw invalid();
  const suffix = inventory.target.startsWith('windows-') ? '.exe' : '';
  for (const name of ['forme-hostd', 'forme-webd', 'forme-gatewayd', 'forme-cli', 'forme-hostctl']) {
    const path = `bin/${name}${suffix}`;
    if (!names.includes(path)) throw invalid();
    const file = await open(join(root, path), 'r');
    try {
      const bytes = Buffer.alloc(1024 * 1024);
      const read = await file.read(bytes, 0, bytes.length, 0);
      if (!binaryMatches(bytes.subarray(0, read.bytesRead), inventory.target)) throw invalid();
    } finally { await file.close(); }
  }
  return inventory;
}

export async function verifyToolchain(path, ids) {
  const config = JSON.parse(await bounded(path, 64 * 1024));
  closed(config, ['schema_version', 'tools']);
  if (config.schema_version !== 1 || !Array.isArray(config.tools) || config.tools.length !== ids.length) throw invalid();
  const tools = new Map();
  for (const tool of config.tools) {
    closed(tool, ['id', 'path', 'sha256', 'version_label']);
    if (!ids.includes(tool.id) || tools.has(tool.id) || !/^[0-9a-f]{64}$/.test(tool.sha256) ||
        typeof tool.version_label !== 'string' || tool.version_label.length > 128 || !tool.version_label.trim() || /[\x00-\x1f]/.test(tool.version_label)) throw invalid();
    await recheckTool(tool);
    tools.set(tool.id, tool);
  }
  return tools;
}

export async function recheckTool(tool) {
  await checked(tool.path);
  const stat = await lstat(tool.path);
  if (await fileDigest(tool.path, stat.size) !== tool.sha256) throw invalid();
}
