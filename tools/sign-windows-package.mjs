#!/usr/bin/env node
// Sign a Windows installer copy and record independent Authenticode evidence.
// This does not install, trust, or publish the package.
import { constants } from 'node:fs';
import { createHash } from 'node:crypto';
import { createReadStream } from 'node:fs';
import { copyFile, lstat, writeFile } from 'node:fs/promises';
import { spawn } from 'node:child_process';
import { dirname, isAbsolute, join, relative, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';

const invalid = (message = 'Invalid Windows package signing input; existing files were retained') =>
  new Error(message);
const stores = new Set(['CurrentUser', 'LocalMachine']);
const statusValues = new Set(['Valid', 'NotSigned', 'NotTrusted', 'HashMismatch', 'UnknownError',
  'NotSupportedFileFormat', 'Incompatible', 'OtherError']);

function absolute(value) {
  return typeof value === 'string' && value.length > 0 && isAbsolute(value) && resolve(value) === value &&
    !/[\x00-\x1f%"'`$]/.test(value);
}

function inside(parent, child) {
  const path = relative(parent, child);
  return path === '' || (!path.startsWith(`..${sep}`) && path !== '..' && !isAbsolute(path));
}

async function checked(path, missing = false) {
  if (!absolute(path)) throw invalid();
  for (let current = path;; current = dirname(current)) {
    try {
      if ((await lstat(current)).isSymbolicLink()) throw invalid();
    } catch (error) {
      if (error.code !== 'ENOENT' || !missing) throw invalid();
    }
    if (dirname(current) === current) return;
  }
}

function normalizeThumbprint(value) {
  if (typeof value !== 'string') throw invalid();
  const normalized = value.replace(/[\s-]/g, '').toUpperCase();
  if (!/^[0-9A-F]{40}$/.test(normalized)) throw invalid('Certificate thumbprint must be exactly 40 hexadecimal characters');
  return normalized;
}

function cleanText(value, limit = 512) {
  if (typeof value !== 'string') return '';
  return value.replace(/[\x00-\x1f\x7f]/g, ' ').slice(0, limit);
}

export function classifyTrust(signatureStatus, signatureType) {
  if (signatureType !== 'Authenticode' || signatureStatus === 'NotSigned') return 'not_signed';
  if (signatureStatus === 'Valid') return 'trusted';
  if (signatureStatus === 'NotTrusted') return 'untrusted';
  return 'unverifiable';
}

export function parseArguments(args) {
  const values = { storeLocation: 'CurrentUser', allowUntrusted: false };
  const supplied = new Set();
  const names = new Map([
    ['--input', 'input'], ['--output', 'output'], ['--certificate-thumbprint', 'certificateThumbprint'],
    ['--store-location', 'storeLocation'],
  ]);
  for (let index = 0; index < args.length; index += 1) {
    const argument = args[index];
    if (argument === '--allow-untrusted') {
      if (values.allowUntrusted) throw invalid();
      values.allowUntrusted = true;
      continue;
    }
    const name = names.get(argument), value = args[index + 1];
    if (!name || supplied.has(name) || !value || value.startsWith('--')) throw invalid();
    values[name] = value;
    supplied.add(name);
    index += 1;
  }
  if (!values.input || !values.output || !values.certificateThumbprint || !stores.has(values.storeLocation)) throw invalid();
  values.certificateThumbprint = normalizeThumbprint(values.certificateThumbprint);
  return values;
}

async function fileDigest(path) {
  await checked(path);
  const before = await lstat(path);
  if (!before.isFile() || before.nlink !== 1 || before.size === 0 || before.size > 2 * 1024 ** 3) throw invalid();
  const hash = createHash('sha256');
  let bytes = 0;
  for await (const chunk of createReadStream(path)) {
    bytes += chunk.length;
    if (bytes > before.size) throw invalid();
    hash.update(chunk);
  }
  const after = await lstat(path);
  if (!after.isFile() || after.nlink !== 1 || after.size !== before.size || bytes !== before.size) throw invalid();
  return { bytes, sha256: `sha256:${hash.digest('hex')}` };
}

function powershellPath() {
  if (process.platform !== 'win32' || !process.env.SystemRoot) throw invalid('Windows PowerShell signing is only available on Windows');
  return join(process.env.SystemRoot, 'System32', 'WindowsPowerShell', 'v1.0', 'powershell.exe');
}

async function invokePowerShell({ packagePath, thumbprint, storeLocation }) {
  const script = join(dirname(fileURLToPath(import.meta.url)), 'windows', 'sign-authenticode.ps1');
  await checked(script);
  const args = ['-NoLogo', '-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', script,
    '-PackagePath', packagePath, '-CertificateThumbprint', thumbprint, '-StoreLocation', storeLocation];
  const environment = {};
  for (const key of ['SystemRoot', 'WINDIR', 'ProgramFiles', 'ProgramW6432', 'USERPROFILE', 'HOMEDRIVE',
    'HOMEPATH', 'LOCALAPPDATA', 'TEMP', 'TMP']) if (process.env[key]) environment[key] = process.env[key];
  environment.PSModulePath = [
    join(process.env.SystemRoot, 'System32', 'WindowsPowerShell', 'v1.0', 'Modules'),
    process.env.ProgramFiles && join(process.env.ProgramFiles, 'WindowsPowerShell', 'Modules'),
  ].filter(Boolean).join(';');
  const child = spawn(powershellPath(), args, { env: environment, stdio: ['ignore', 'pipe', 'pipe'], windowsHide: true, shell: false });
  const stdout = [], stderr = [];
  let size = 0;
  for (const stream of [child.stdout, child.stderr]) stream.on('data', (chunk) => {
    size += chunk.length;
    if (size <= 1024 * 1024) (stream === child.stdout ? stdout : stderr).push(chunk);
    else child.kill();
  });
  const code = await new Promise((resolveCode, reject) => {
    child.once('error', reject);
    child.once('close', resolveCode);
  });
  const output = Buffer.concat(stdout).toString('utf8').trim();
  if (code !== 0 || size > 1024 * 1024) throw invalid(`Windows Authenticode operation failed: ${cleanText(Buffer.concat(stderr).toString('utf8'))}`);
  let observation;
  try { observation = JSON.parse(output); } catch { throw invalid('Windows Authenticode observation was not valid JSON'); }
  if (!observation || typeof observation !== 'object' || Array.isArray(observation)) throw invalid();
  if (!statusValues.has(observation.signature_status) || !statusValues.has(observation.signing_status) ||
      observation.signature_status === 'NotSigned' || observation.signing_status !== observation.signature_status ||
      observation.signature_type !== 'Authenticode' ||
      normalizeThumbprint(observation.signer_thumbprint) !== thumbprint) throw invalid('Signer observation did not match the requested certificate');
  return observation;
}

export async function signWindowsPackage(config) {
  if (process.platform !== 'win32') throw invalid('Windows Authenticode signing is only available on Windows');
  if (!config || typeof config !== 'object' || Array.isArray(config) ||
      Object.keys(config).some((key) => !['input', 'output', 'certificateThumbprint', 'storeLocation', 'allowUntrusted'].includes(key))) throw invalid();
  const input = config.input, output = config.output;
  const thumbprint = normalizeThumbprint(config.certificateThumbprint);
  const storeLocation = config.storeLocation ?? 'CurrentUser';
  const allowUntrusted = config.allowUntrusted === true;
  if (!absolute(input) || !absolute(output) || resolve(input) === resolve(output) || !stores.has(storeLocation)) throw invalid();
  await checked(input);
  await checked(dirname(output));
  await checked(output, true);
  try { await lstat(output); throw invalid('Refusing to replace an existing signed-package output'); }
  catch (error) { if (error.code !== 'ENOENT') throw error; }
  if (inside(input, output) || inside(output, input)) throw invalid('Input and output paths must be separate');
  const source = await fileDigest(input);
  await copyFile(input, output, constants.COPYFILE_EXCL);
  if ((await fileDigest(output)).sha256 !== source.sha256) throw invalid('Copied package did not retain the source digest');
  const observation = await invokePowerShell({ packagePath: output, thumbprint, storeLocation });
  const signed = await fileDigest(output);
  const trustStatus = classifyTrust(observation.signature_status, observation.signature_type);
  const receipt = {
    schema_version: 1,
    source_package: { name: input.split(/[\\/]/).at(-1), bytes: source.bytes, sha256: source.sha256 },
    signed_package: { name: output.split(/[\\/]/).at(-1), bytes: signed.bytes, sha256: signed.sha256 },
    certificate: { store_location: storeLocation, thumbprint, subject: cleanText(observation.certificate_subject), issuer: cleanText(observation.certificate_issuer) },
    signature_type: observation.signature_type,
    signature_status: observation.signature_status,
    trust_status: trustStatus,
    status_message: cleanText(observation.status_message, 1024),
    native_package_signature: trustStatus === 'trusted' ? 'authenticode_valid' :
      trustStatus === 'not_signed' ? 'not_signed' : `authenticode_present_${trustStatus}`,
    native_installation: 'not_performed',
    release_ready: false,
  };
  const receiptPath = `${output}.signing-receipt.json`;
  await checked(receiptPath, true);
  try { await lstat(receiptPath); throw invalid('Refusing to replace an existing native signing receipt'); }
  catch (error) { if (error.code !== 'ENOENT') throw error; }
  await writeFile(receiptPath, `${JSON.stringify(receipt, null, 2)}\n`, { flag: 'wx', mode: 0o600 });
  if (trustStatus !== 'trusted' && !allowUntrusted) throw invalid(`Authenticode trust status is ${trustStatus}; use --allow-untrusted only for local evidence`);
  return receipt;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const receipt = await signWindowsPackage(parseArguments(process.argv.slice(2)));
    console.log(JSON.stringify(receipt));
  } catch (error) {
    console.error(`Windows package signing failed: ${cleanText(error.message)}`);
    process.exitCode = 1;
  }
}
