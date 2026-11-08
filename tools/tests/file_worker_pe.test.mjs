import assert from 'node:assert/strict';
import { test } from 'node:test';
import { fileWorkerImports } from '../native-package/file-worker-pe.mjs';
import { fileWorkerPe } from './fixtures/file_worker_pe.mjs';

test('headless import gate accepts only bounded OS runtime dependencies', () => {
  const names = ['KERNEL32.dll', 'ntdll.dll', 'api-ms-win-crt-runtime-l1-1-0.dll'];
  assert.deepEqual(fileWorkerImports(fileWorkerPe(names)), names.map((name) => name.toLowerCase()));
  for (const name of ['user32.dll', 'gdi32.dll', 'shell32.dll', 'd3d11.dll', 'dwmapi.dll',
    'api-ms-win-shcore-scaling-l1-1-1.dll', 'foreign.dll', '../kernel32.dll']) {
    assert.throws(() => fileWorkerImports(fileWorkerPe([name])));
  }
});

test('ambiguous PE layout, missing terminators and delayed imports fail closed', () => {
  const optional = 152, section = 392;
  const changes = [
    (bytes) => bytes.writeUInt32LE(0xfffffff0, 60),
    (bytes) => bytes.writeUInt16LE(1, optional + 68),
    (bytes) => bytes.writeUInt32LE(0x1000, optional + 216),
    (bytes) => bytes.writeUInt32LE(20, optional + 220),
    (bytes) => bytes.writeUInt32LE(0xffffffff, optional + 120),
    (bytes) => bytes.writeUInt32LE(20, optional + 124),
    (bytes) => bytes.writeUInt32LE(0xffffffff, 512 + 12),
    (bytes) => bytes.writeUInt32LE(0xffffffff, 512 + 16),
    (bytes) => bytes.writeUInt32LE(0, section + 20),
    (bytes) => bytes.fill(65, 1024, 1152),
    (bytes) => bytes.writeUInt32LE(1, 532),
    (bytes) => { bytes.writeUInt16LE(2, 134); bytes.copy(bytes, section + 40, section, section + 40); },
  ];
  for (const change of changes) {
    const bytes = fileWorkerPe(); change(bytes);
    assert.throws(() => fileWorkerImports(bytes));
  }
  for (const length of [0, 63, 150, 393, 1025]) {
    assert.throws(() => fileWorkerImports(fileWorkerPe().subarray(0, length)));
  }
});
