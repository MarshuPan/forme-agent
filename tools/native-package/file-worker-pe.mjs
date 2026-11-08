// Inspect project-built PE images before packaging. This is not a PE loader.
// Restrict the file entry to OS runtime DLLs; desktop libraries belong to the
// separate executor image. Unsupported or ambiguous input fails closed.
const invalid = () => new Error('File worker PE imports are invalid or require non-runtime DLLs');
const runtimeDlls = new Set([
  'kernel32.dll', 'ntdll.dll', 'bcryptprimitives.dll', 'bcrypt.dll',
  'advapi32.dll', 'userenv.dll', 'ws2_32.dll',
]);

export function fileWorkerImports(bytes) {
  const range = (offset, size) => {
    if (!Number.isSafeInteger(offset) || !Number.isSafeInteger(size) || offset < 0 ||
        size < 0 || offset > bytes.length - size) throw invalid();
    return offset;
  };
  const u16 = (offset) => bytes.readUInt16LE(range(offset, 2));
  const u32 = (offset) => bytes.readUInt32LE(range(offset, 4));
  if (!Buffer.isBuffer(bytes) || bytes.length < 64 || bytes.length > 512 * 1024 ** 2 ||
      bytes.toString('ascii', 0, 2) !== 'MZ') throw invalid();
  const pe = u32(60);
  range(pe, 24);
  if (pe < 64 || u32(pe) !== 0x4550 || u16(pe + 4) !== 0x8664 ||
      (u16(pe + 22) & 0x2002) !== 2) throw invalid();
  const count = u16(pe + 6), optionalSize = u16(pe + 20), optional = pe + 24;
  if (count < 1 || count > 96 || optionalSize < 240 || optionalSize > 4096) throw invalid();
  range(optional, optionalSize);
  if (u16(optional) !== 0x20b || u16(optional + 68) !== 3) throw invalid();
  const directories = u32(optional + 108);
  if (directories < 14 || directories > 16 || 112 + directories * 8 > optionalSize) throw invalid();
  // Delay imports could bypass the normal import table. No current file
  // worker uses them; support must be reviewed explicitly before enabling.
  if (u32(optional + 112 + 13 * 8) !== 0 || u32(optional + 116 + 13 * 8) !== 0) throw invalid();
  const headers = u32(optional + 60), table = optional + optionalSize;
  range(0, headers);
  range(table, count * 40);
  if (headers < table + count * 40) throw invalid();
  const sections = [];
  for (let index = 0; index < count; index++) {
    const offset = table + index * 40;
    const virtualSize = u32(offset + 8), rva = u32(offset + 12);
    const size = u32(offset + 16), raw = u32(offset + 20);
    const span = Math.max(virtualSize, size);
    if (rva < headers || span === 0 || rva + span > 0x100000000 ||
        (size > 0 && raw < headers)) throw invalid();
    range(raw, size);
    for (const previous of sections) {
      if ((rva < previous.rva + previous.span && previous.rva < rva + span) ||
          (size > 0 && previous.size > 0 && raw < previous.raw + previous.size &&
            previous.raw < raw + size)) throw invalid();
    }
    sections.push({ rva, span, size, raw });
  }
  const map = (rva, size) => {
    const section = sections.find((entry) => rva >= entry.rva &&
      rva - entry.rva <= entry.size - size);
    if (!section || size < 1) throw invalid();
    return range(section.raw + rva - section.rva, size);
  };
  const nameAt = (rva) => {
    let name = '';
    for (let offset = 0; offset < 128; offset++) {
      const byte = bytes[map(rva + offset, 1)];
      if (byte === 0) {
        if (!/^[A-Za-z0-9_.-]+\.dll$/i.test(name)) throw invalid();
        return name.toLowerCase();
      }
      if (byte < 0x21 || byte > 0x7e) throw invalid();
      name += String.fromCharCode(byte);
    }
    throw invalid();
  };
  const importRva = u32(optional + 120), importSize = u32(optional + 124);
  if (importRva === 0 || importSize < 40 || importSize > 64 * 1024) throw invalid();
  map(importRva, importSize);
  const names = [];
  for (let index = 0; index < 1024 && (index + 1) * 20 <= importSize; index++) {
    const offset = map(importRva + index * 20, 20);
    const descriptor = Array.from({ length: 5 }, (_, word) => u32(offset + word * 4));
    if (descriptor.every((word) => word === 0)) {
      if (names.length === 0) throw invalid();
      return names;
    }
    // Require a file-backed IAT and a valid DLL name, even though this gate
    // does not interpret imported functions or execute the image.
    map(descriptor[4], 8);
    const name = nameAt(descriptor[3]);
    if (!runtimeDlls.has(name) &&
        !/^api-ms-win-(?:core|crt)-[a-z0-9-]+\.dll$/.test(name)) throw invalid();
    names.push(name);
  }
  throw invalid();
}
