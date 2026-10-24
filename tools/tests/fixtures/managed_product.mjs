// Repository-owned disposable installation. Never loads .forme or user keys.
import assert from 'node:assert/strict';
import { spawn, execFileSync } from 'node:child_process';
import { once } from 'node:events';
import { promises as fs } from 'node:fs';
import { createServer } from 'node:net';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const repo = fileURLToPath(new URL('../../../', import.meta.url));
const suffix = process.platform === 'win32' ? '.exe' : '';
export const readJson = async (file) => JSON.parse(await fs.readFile(file, 'utf8'));
export const writeJson = (file, value) => fs.writeFile(file, JSON.stringify(value), { mode: 0o600 });

async function allocatePorts(count) {
  const servers = [];
  try {
    for (let i = 0; i < count; i++) {
      const server = createServer(); servers.push(server);
      server.listen(0, '127.0.0.1'); await once(server, 'listening');
    }
    return servers.map((server) => `127.0.0.1:${server.address().port}`);
  } finally { await Promise.all(servers.map((server) => new Promise((resolve) => server.close(resolve)))); }
}

export async function createManagedProduct({ binaries = path.join(repo, 'target/debug') } = {}) {
  assert.ok(path.isAbsolute(binaries), 'fixture binary directory must be absolute');
  const executable = (name) => path.join(binaries, name + suffix);
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'forme-config-process-'));
  const processes = new Set();
  const environment = { TEMP: root, TMP: root };
  if (process.env.SystemRoot) environment.SystemRoot = process.env.SystemRoot;
  const run = (name, args) => JSON.parse(execFileSync(executable(name), args,
    { cwd: root, env: environment, timeout: 30000, maxBuffer: 256 * 1024, windowsHide: true, stdio: ['ignore', 'pipe', 'pipe'] }));
  const cleanup = async () => {
    for (const process of [...processes].reverse()) await stop(process);
    const resolved = path.resolve(root);
    assert.equal(path.dirname(resolved), path.resolve(os.tmpdir()));
    assert.ok(path.basename(resolved).startsWith('forme-config-process-'));
    assert.equal((await fs.lstat(resolved)).isSymbolicLink(), false);
    await fs.rm(resolved, { recursive: true });
  };
  async function start(name, config, announcement) {
    const child = spawn(executable(name), ['--config', config], { cwd: root, env: environment,
      windowsHide: true, stdio: ['ignore', 'pipe', 'pipe'] });
    processes.add(child);
    let output = '', diagnostic = '';
    // Do not publish process logs: even fixture secrets use the private channel.
    child.stderr.on('data', (bytes) => { diagnostic = (diagnostic + bytes.toString()).slice(-8192); });
    try {
      await new Promise((resolve, reject) => {
        const timer = setTimeout(() => { finish(); reject(new Error(`${name} readiness timeout`)); }, 30000);
        const finish = () => { clearTimeout(timer); child.off('exit', exited); child.off('error', failed); };
        const failed = () => { finish(); reject(new Error(`${name} failed to spawn`)); };
        const exited = (code) => { finish();
          // Only this disposable, repository-owned fixture is eligible for diagnostics.
          const safe = diagnostic.replaceAll(root, '[fixture-root]').replace(/fixture-[A-Za-z0-9_-]*key[A-Za-z0-9_-]*/g, '[fixture-key]');
          reject(new Error(`${name} exited before readiness (${code}): ${safe}`)); };
        child.once('error', failed); child.once('exit', exited);
        child.stdout.on('data', (bytes) => {
          output = (output + bytes.toString()).slice(-8192);
          if (output.includes(announcement)) { finish(); resolve(); }
        });
      });
      return child;
    } catch (error) { await stop(child); throw error; }
  }
  async function stop(child) {
    if (child.exitCode === null && child.signalCode === null) {
      const ended = once(child, 'exit');
      child.kill();
      const timeout = setTimeout(() => child.kill('SIGKILL'), 5000);
      try { await ended; } finally { clearTimeout(timeout); }
    }
    processes.delete(child);
  }
  try {
    const targets = { win32: 'windows_x86_64', linux: `linux_${process.arch === 'arm64' ? 'aarch64' : 'x86_64'}`,
      darwin: `macos_${process.arch === 'arm64' ? 'aarch64' : 'x86_64'}` };
    assert.ok(targets[process.platform], 'unsupported fixture platform');
    const prepared = run('forme-hostctl', ['prepare-installation', '--install-root', path.join(root, 'install'),
      '--state-root', path.join(root, 'state'), '--config-root', path.join(root, 'config'), '--log-root', path.join(root, 'logs'),
      '--ipc-root', path.join(root, 'ipc'), '--target', targets[process.platform], '--release-id', 'fixture-config-1',
      '--release-digest', `sha256:${'a'.repeat(64)}`]);
    const [hostAddress, agentAddress, configurationAddress, coreAddress, webAddress] = await allocatePorts(5);
    const host = await readJson(prepared.host_config_path);
    Object.assign(host, { host_bind: hostAddress, agent_execution_bind: agentAddress, configuration_bind: configurationAddress });
    await writeJson(prepared.host_config_path, host);
    const web = await readJson(prepared.web_config_path);
    web.bind = webAddress; web.bootstrap_bridge.address = hostAddress;
    web.configuration_bridge.address = configurationAddress; web.core_bridge.address = coreAddress;
    await writeJson(prepared.web_config_path, web);
    const template = await readJson(host.product_config_template_path);
    template.core_bind = coreAddress; template.web_bind = webAddress;
    template.cognition_tick = 600000; template.authority_lease = 3000;
    if (web.network_config_path) {
      await writeJson(web.network_config_path, { schema_version: 1, bind: webAddress,
        allowed_hosts: [], tls: null, firewall: null });
    }
    await writeJson(host.product_config_template_path, template);
    const cookieJar = new Map();
    let csrf;
    const origin = `http://${webAddress}`;
    async function request(route, { method = 'GET', body, key, expected = 200, headers = {}, text = false } = {}) {
      const result = await fetch(origin + route, { method, signal: AbortSignal.timeout(30000), headers: {
        Cookie: [...cookieJar].map(([name, value]) => `${name}=${value}`).join('; '),
        ...(method !== 'GET' ? { Origin: origin, 'Content-Type': 'application/json',
          'Idempotency-Key': key ?? 'fixture:request', ...(csrf ? { 'X-Forme-Csrf': csrf } : {}) } : {}), ...headers,
      }, body: body === undefined ? undefined : JSON.stringify(body) });
      for (const cookie of result.headers.getSetCookie()) {
        const pair = cookie.split(';', 1)[0]; const at = pair.indexOf('=');
        cookieJar.set(pair.slice(0, at), pair.slice(at + 1));
      }
      assert.equal(result.status, expected, `${method} ${route} returned an unexpected status`);
      if (text) return result.text();
      const value = await result.json();
      if (value.csrf) csrf = value.csrf;
      return value;
    }
    return { root, prepared, host, template, origin, coreAddress, start, stop, cleanup, request,
      startHost: () => start('forme-hostd', prepared.host_config_path, `FORME_CONFIGURATION_URL=http://${configurationAddress}`),
      startWeb: () => start('forme-webd', prepared.web_config_path, `FORME_WEB_URL=${origin}`),
      startCore: () => start('forme-gatewayd', template.core_config_path, `FORME_GATEWAY_URL=http://${coreAddress}`),
      proof: (operation) => run('forme-hostctl', ['bootstrap-proof', '--address', hostAddress,
        '--credential-file', host.maintenance_credential_path, '--operation', operation]).proof,
    };
  } catch (error) { await cleanup(); throw error; }
}
