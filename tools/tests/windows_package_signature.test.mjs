import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { mkdtemp, readFile, readdir, rm, writeFile } from 'node:fs/promises';
import { dirname, join, resolve } from 'node:path';
import { tmpdir } from 'node:os';
import { test } from 'node:test';
import { classifyTrust, parseArguments, signWindowsPackage } from '../sign-windows-package.mjs';

test('Windows signer parses explicit certificate and trust options', () => {
  const parsed = parseArguments(['--input', resolve('input.exe'), '--output', resolve('output.exe'),
    '--certificate-thumbprint', '84f1194b-de2a-6476-9b52-d04e5e9a2b71-49605d5e', '--allow-untrusted']);
  assert.equal(parsed.certificateThumbprint, '84F1194BDE2A64769B52D04E5E9A2B7149605D5E');
  assert.equal(parsed.storeLocation, 'CurrentUser');
  assert.equal(parsed.allowUntrusted, true);
  assert.throws(() => parseArguments(['--input', resolve('input.exe'), '--output', resolve('output.exe'),
    '--certificate-thumbprint', 'not-a-thumbprint']));
  assert.throws(() => parseArguments(['--input', resolve('input.exe'), '--output', resolve('output.exe'),
    '--certificate-thumbprint', '84f1194bde2a64769b52d04e5e9a2b7149605d5e', '--allow-untrusted', '--allow-untrusted']));
});

test('trust classification never upgrades unknown or untrusted Authenticode to trusted', () => {
  assert.equal(classifyTrust('Valid', 'Authenticode'), 'trusted');
  assert.equal(classifyTrust('NotTrusted', 'Authenticode'), 'untrusted');
  assert.equal(classifyTrust('UnknownError', 'Authenticode'), 'unverifiable');
  assert.equal(classifyTrust('Valid', 'None'), 'not_signed');
  assert.equal(classifyTrust('NotSigned', 'Authenticode'), 'not_signed');
});

test('signer rejects unsafe overlap and preserves existing outputs before invoking PowerShell', async (t) => {
  if (process.platform !== 'win32') return t.skip('Windows-only signer');
  const root = await mkdtemp(join(tmpdir(), 'forme-windows-signature-'));
  t.after(() => rm(root, { recursive: true, force: true }));
  const input = join(root, 'input.exe'), output = join(root, 'output.exe');
  await writeFile(input, 'fixture package');
  const config = { input, output, certificateThumbprint: '84F1194BDE2A64769B52D04E5E9A2B7149605D5E', allowUntrusted: true };
  await assert.rejects(signWindowsPackage({ ...config, output: input }));
  await writeFile(output, 'existing');
  await assert.rejects(signWindowsPackage(config));
  assert.equal(await readFile(output, 'utf8'), 'existing');
  assert.deepEqual(await readdir(dirname(output)), ['input.exe', 'output.exe']);
});

test('real local certificate signing is opt-in and records the exact output evidence', async (t) => {
  if (process.platform !== 'win32' || !process.env.FORME_WINDOWS_PACKAGE_TEST_THUMBPRINT || !process.env.FORME_WINDOWS_PACKAGE_TEST_INPUT) {
    return t.skip('requires an explicit local Windows certificate and package input');
  }
  const root = await mkdtemp(join(tmpdir(), 'forme-windows-signature-live-'));
  t.after(() => rm(root, { recursive: true, force: true }));
  const output = join(root, 'signed.exe');
  const receipt = await signWindowsPackage({ input: resolve(process.env.FORME_WINDOWS_PACKAGE_TEST_INPUT), output,
    certificateThumbprint: process.env.FORME_WINDOWS_PACKAGE_TEST_THUMBPRINT, allowUntrusted: true });
  assert.equal(receipt.signature_type, 'Authenticode');
  assert.equal(receipt.native_installation, 'not_performed');
  assert.equal(receipt.release_ready, false);
  assert.equal(receipt.signed_package.sha256, `sha256:${createHash('sha256').update(await readFile(output)).digest('hex')}`);
  assert.deepEqual(JSON.parse(await readFile(join(root, 'native-signing-receipt.json'), 'utf8')), receipt);
});
