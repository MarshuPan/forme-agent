// A forged PATH identity command cannot grant local credential-reset authority.
import test from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync, spawnSync } from 'node:child_process';
import { promises as fs } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

test('local recovery uses OS authority and never executes a PATH-supplied identity command', { timeout: 60000 }, async (t) => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'forme-local-authority-'));
  t.after(async () => {
    assert.equal(path.dirname(path.resolve(root)), path.resolve(os.tmpdir()));
    assert.ok(path.basename(root).startsWith('forme-local-authority-'));
    assert.equal((await fs.lstat(root)).isSymbolicLink(), false);
    await fs.rm(root, { recursive: true });
  });
  const windows = process.platform === 'win32';
  const fake = path.join(root, windows ? 'whoami.exe' : 'id');
  execFileSync('rustc', [fileURLToPath(new URL('./fixtures/local_authority_spoof.rs', import.meta.url)), '-o', fake],
    { windowsHide: true, timeout: 30000, stdio: 'pipe' });
  const marker = path.join(root, 'called.txt');
  const environment = { PATH: root, FORME_AUTHORITY_SPOOF_MARKER: marker };
  if (process.env.SystemRoot) environment.SystemRoot = process.env.SystemRoot;
  let elevated;
  if (windows) {
    const shell = path.join(process.env.SystemRoot, 'System32/WindowsPowerShell/v1.0/powershell.exe');
    elevated = execFileSync(shell, ['-NoProfile', '-Command',
      '([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)'],
    { windowsHide: true, timeout: 15000, encoding: 'utf8' }).trim() === 'True';
  } else elevated = process.geteuid() === 0;
  const executable = fileURLToPath(new URL(`../../target/debug/forme-hostctl${windows ? '.exe' : ''}`, import.meta.url));
  const result = spawnSync(executable, ['recover-login', '--web-config', path.join(root, 'absent-config.json'),
    '--operation', 'recovery:negative-fixture', '--password-stdin'],
  { cwd: root, env: environment, input: 'fixture-only-password', encoding: 'utf8', windowsHide: true, timeout: 15000 });
  assert.equal(result.error, undefined);
  assert.equal(result.status, 1);
  assert.equal(result.stdout, '');
  await assert.rejects(fs.stat(marker), { code: 'ENOENT' });
  if (!elevated) assert.match(result.stderr, /local administrator authority is required/);
  else assert.doesNotMatch(result.stderr, /local administrator authority is required/);
  t.diagnostic(`Actual elevated authority: ${elevated}; no account or identity store was opened or reset.`);
});
