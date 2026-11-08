// Owned structural PE64 fixture. Never executed or used as native evidence.
export function fileWorkerPe(imports = ['kernel32.dll']) {
  const bytes = Buffer.alloc(2048), pe = 128, optional = pe + 24, section = optional + 240;
  bytes.write('MZ'); bytes.writeUInt32LE(pe, 60); bytes.writeUInt32LE(0x4550, pe);
  bytes.writeUInt16LE(0x8664, pe + 4); bytes.writeUInt16LE(1, pe + 6);
  bytes.writeUInt16LE(240, pe + 20); bytes.writeUInt16LE(2, pe + 22);
  bytes.writeUInt16LE(0x20b, optional); bytes.writeUInt16LE(3, optional + 68);
  bytes.writeUInt32LE(512, optional + 60); bytes.writeUInt32LE(16, optional + 108);
  bytes.writeUInt32LE(0x1000, optional + 120);
  bytes.writeUInt32LE((imports.length + 1) * 20, optional + 124);
  bytes.write('.rdata', section); bytes.writeUInt32LE(1536, section + 8);
  bytes.writeUInt32LE(0x1000, section + 12); bytes.writeUInt32LE(1536, section + 16);
  bytes.writeUInt32LE(512, section + 20);
  let name = 1024;
  for (let index = 0; index < imports.length; index++) {
    const offset = 512 + index * 20;
    bytes.writeUInt32LE(0x1400, offset);
    bytes.writeUInt32LE(0x1000 + name - 512, offset + 12);
    bytes.writeUInt32LE(0x1420, offset + 16);
    name += bytes.write(imports[index] + '\0', name);
  }
  return bytes;
}
