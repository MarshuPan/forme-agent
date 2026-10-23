// Shell syntax and finite removal control flow, never native account/IP evidence.
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { dirname, join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { test } from 'node:test';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const scripts = [
  'packaging/posix/service-access.sh', 'packaging/linux/forme.preinst',
  'packaging/linux/forme.postinst', 'packaging/linux/forme.prerm',
  'packaging/macos/scripts/preinstall', 'packaging/macos/scripts/postinstall',
];

test('all original POSIX install hooks have valid shell syntax', async () => {
  for (const script of scripts) {
    const source = await readFile(join(root, script), 'utf8');
    const result = spawnSync('bash', ['-n'], { input: source, encoding: 'utf8' });
    assert.equal(result.status, 0, `${script}: ${result.stderr}`);
  }
});

async function removal(fail = '') {
  let source = await readFile(join(root, 'packaging/linux/forme.prerm'), 'utf8');
  // Shim only the OS boundary; execute the original script and its fail/stop
  // branches. The fixture never controls a host systemctl or writes /etc.
  source = `systemctl() {
  printf 'ACTION:%s\n' "$*" >&2
  if [ "$*" = '${fail}' ]; then return 7; fi
  if [ "$1" = show ]; then printf '%s\n' inactive; fi
}\n${source}`;
  const result = spawnSync('bash', ['-s', 'remove'], {
    input: source, encoding: 'utf8',
  });
  return { result, actions: result.stderr.split(/\r?\n/).filter((line) => line.startsWith('ACTION:')).map((line) => line.slice(7)) };
}

test('package removal stops and observes Brain before Web and Host', async () => {
  const { result, actions } = await removal();
  assert.equal(result.status, 0, result.stderr);
  assert.deepEqual(actions, ['forme-brain', 'forme-web', 'forme-host'].flatMap((service) => [
    `stop ${service}.service`, `show --property=ActiveState --value ${service}.service`, `disable ${service}.service`,
  ]));
});

test('uncertain stop prevents all subsequent service removal', async () => {
  const { result, actions } = await removal('stop forme-brain.service');
  assert.notEqual(result.status, 0);
  assert.deepEqual(actions, ['stop forme-brain.service']);
});

test('failed state observation prevents disable and later services', async () => {
  const failure = 'show --property=ActiveState --value forme-web.service';
  const { result, actions } = await removal(failure);
  assert.notEqual(result.status, 0);
  assert.deepEqual(actions.slice(-2), ['stop forme-web.service', failure]);
  assert(!actions.some((line) => line.includes('forme-host') || line === 'disable forme-web.service'));
});

test('service mounts keep configuration publication in Host and global keys out of Brain', async () => {
  const host = await readFile(join(root, 'packaging/linux/forme-host.service'), 'utf8');
  const brain = await readFile(join(root, 'packaging/linux/forme-brain.service'), 'utf8');
  const web = await readFile(join(root, 'packaging/linux/forme-web.service'), 'utf8');
  assert.match(host, /^ReadWritePaths=.*\/etc\/forme .*\/keys\/model-credentials .*\/backups/m);
  for (const service of [brain, web]) {
    const writable = service.split('\n').find((line) => line.startsWith('ReadWritePaths='));
    assert(writable);
    assert(!writable.includes('/keys') && !writable.includes('/etc/forme') && !writable.includes('/host-state'));
    assert.match(service, /^SupplementaryGroups=forme-bridge$/m);
  }
});

test('POSIX service templates never declare an unbounded recovery loop', async () => {
  for (const name of ['forme-brain.service', 'forme-host.service', 'forme-web.service']) {
    const service = await readFile(join(root, 'packaging/linux', name), 'utf8');
    assert.match(service, /^Restart=on-failure$/m);
    assert.match(service, /^RestartSec=5s$/m);
    assert.match(service, /^StartLimitIntervalSec=86400$/m);
    assert.match(service, /^StartLimitBurst=4$/m);
    assert.doesNotMatch(service, /^Restart=always$/m);
  }
  for (const name of ['com.forme.brain.plist', 'com.forme.host.plist', 'com.forme.web.plist']) {
    const plist = await readFile(join(root, 'packaging/macos', name), 'utf8');
    assert.doesNotMatch(plist, /<key>KeepAlive<\/key>\s*<true\/>/);
    assert.match(plist, /launchd has no finite failure-count\/escalating-delay primitive/);
  }
});
