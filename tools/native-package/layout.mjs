// Fixed native payload/command plans. No shell command accepts Agent input.
import { constants } from 'node:fs';
import { chmod, copyFile, mkdir, readFile, writeFile } from 'node:fs/promises';
import { dirname, join } from 'node:path';
import { fileDigest, invalid } from './inputs.mjs';

const copiedMode = (path) => path.startsWith('bin/') || path.endsWith('.sh') || /\/(preinstall|postinstall)$/.test(path) ? 0o755 : 0o644;

export async function stagePayload(bundle, destination, inventory) {
  const entries = [...inventory.files, { path: 'bundle-inventory.json' }, { path: 'SHA256SUMS' }];
  for (const entry of entries) {
    const source = join(bundle, entry.path), target = join(destination, entry.path);
    await mkdir(dirname(target), { recursive: true, mode: 0o755 });
    if (entry.sha256 && await fileDigest(source, entry.bytes) !== entry.sha256) throw invalid();
    await copyFile(source, target, constants.COPYFILE_EXCL);
    await chmod(target, copiedMode(entry.path));
    if (entry.sha256 && await fileDigest(target, entry.bytes) !== entry.sha256) throw invalid();
  }
}

async function textFile(path, text, mode = 0o644) {
  await mkdir(dirname(path), { recursive: true, mode: 0o755 });
  await writeFile(path, text.replaceAll('\r\n', '\n'), { flag: 'wx', mode });
}

export function nativeCommands(format, paths, version) {
  const { payload, work, artifacts, packageName } = paths;
  const bundle = paths.compilerBundle ?? paths.bundle;
  if (format === 'exe') return [{ id: 'iscc', args: [`/DProductVersion=${version}`, `/DReleaseRoot=${bundle}`, `/O${artifacts}`, `/F${packageName}`, join(bundle, 'packaging/windows/forme.iss')] }];
  if (format === 'deb') return [{ id: 'dpkg-deb', args: ['--root-owner-group', '--uniform-compression', '--build', payload, join(artifacts, `${packageName}.deb`)] }];
  if (format === 'rpm') return [{ id: 'rpmbuild', args: ['--define', `_topdir ${work}`, '--target', 'x86_64', '-bb', join(work, 'SPECS/forme.spec')] }];
  if (format === 'pkg') return [
    { id: 'pkgbuild', args: ['--root', payload, '--identifier', 'com.forme.product', '--version', version, '--install-location', '/', '--scripts', join(work, 'scripts'), join(work, 'forme-product.pkg')] },
    { id: 'productbuild', args: ['--distribution', join(work, 'Distribution.xml'), '--package-path', work, join(artifacts, `${packageName}.pkg`)] },
  ];
  throw invalid();
}

export async function prepareLayout(format, paths, inventory, version) {
  await mkdir(paths.work, { mode: 0o755 });
  await mkdir(paths.artifacts, { mode: 0o755 });
  if (format === 'exe') {
    await stagePayload(paths.bundle, paths.compilerBundle, inventory);
    return paths.compilerBundle;
  }
  await mkdir(paths.payload, { recursive: true, mode: 0o755 });
  const program = format === 'pkg' ? join(paths.payload, 'Library/ForMe') : join(paths.payload, 'opt/forme');
  await stagePayload(paths.bundle, program, inventory);
  if (format === 'deb' || format === 'rpm') {
    for (const name of ['host', 'web', 'brain']) {
      await textFile(join(paths.payload, `usr/lib/systemd/system/forme-${name}.service`), await readFile(join(paths.bundle, `packaging/linux/forme-${name}.service`), 'utf8'));
    }
    await mkdir(join(paths.payload, 'etc/forme'), { recursive: true, mode: 0o700 });
  }
  if (format === 'deb') {
    const control = (await readFile(join(paths.bundle, 'packaging/linux/forme.control'), 'utf8'))
      .replace(/^Version:.*$/m, `Version: ${version}`).replace(/^Architecture:.*$/m, `Architecture: ${inventory.target.endsWith('arm64') ? 'arm64' : 'amd64'}`);
    await textFile(join(paths.payload, 'DEBIAN/control'), control);
    for (const name of ['preinst', 'postinst', 'prerm']) {
      let text = await readFile(join(paths.bundle, `packaging/linux/forme.${name}`), 'utf8');
      if (name === 'preinst') {
        const arch = inventory.target.endsWith('arm64') ? 'aarch64' : 'x86_64';
        text = text.replace('set -eu', `set -eu\n. /etc/os-release\n[ "$ID" = ubuntu ] && [ "$VERSION_ID" = 24.04 ] && [ "$(uname -m)" = ${arch} ] || exit 1`);
      }
      await textFile(join(paths.payload, 'DEBIAN', name), text, 0o755);
    }
  } else if (format === 'rpm') {
    for (const directory of ['BUILD', 'BUILDROOT', 'SOURCES', 'SPECS', 'SRPMS', 'RPMS']) await mkdir(join(paths.work, directory), { mode: 0o755 });
    let spec = (await readFile(join(paths.bundle, 'packaging/linux/forme.spec'), 'utf8')).replace(/^Version:.*$/m, `Version: ${version}`);
    spec = spec.replace('%files', `%install\nmkdir -p "%{buildroot}"\ncp -a "${paths.payload}/." "%{buildroot}/"\n\n%files`);
    await textFile(join(paths.work, 'SPECS/forme.spec'), spec);
  } else if (format === 'pkg') {
    for (const name of ['host', 'web', 'brain']) {
      await textFile(join(paths.payload, `Library/LaunchDaemons/com.forme.${name}.plist`), await readFile(join(paths.bundle, `packaging/macos/com.forme.${name}.plist`), 'utf8'));
    }
    for (const name of ['preinstall', 'postinstall']) {
      let source = await readFile(join(paths.bundle, 'packaging/macos/scripts', name), 'utf8');
      if (name === 'preinstall') {
        const arch = inventory.target.endsWith('arm64') ? 'arm64' : 'x86_64';
        source = source.replace('set -eu', `set -eu\n[ "$(uname -m)" = ${arch} ] || exit 1`);
      }
      await textFile(join(paths.work, 'scripts', name), source, 0o755);
    }
    const distribution = (await readFile(join(paths.bundle, 'packaging/macos/Distribution.xml'), 'utf8')).replace('version="0.0.1"', `version="${version}"`);
    await textFile(join(paths.work, 'Distribution.xml'), distribution);
  }
  return program;
}
