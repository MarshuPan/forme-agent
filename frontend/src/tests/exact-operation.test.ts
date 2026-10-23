import assert from 'node:assert/strict';
import { test } from 'node:test';
import { ExactOperation } from '../exact-operation.ts';

test('a lost response locks the original payload and key; refresh causes no dispatch', async () => {
  const operation = new ExactOperation<{ summary: string }>();
  const form = { summary: 'original' };
  const calls: unknown[] = [];
  await assert.rejects(operation.run(() => ({ key: 'one', payload: form }), async (request) => {
    calls.push(request); throw new Error('response lost');
  }));
  form.summary = 'edited while uncertain';
  assert.equal(operation.key, 'one');
  assert.equal(operation.pending, true);
  assert.equal(calls.length, 1);
  assert.throws(() => operation.releaseConflict());
  await operation.run(() => { throw new Error('must not generate a new key'); }, async (request) => { calls.push(request); });
  assert.deepEqual(calls, [{ key: 'one', payload: { summary: 'original' } }, { key: 'one', payload: { summary: 'original' } }]);
  assert.equal(operation.pending, false);
});

test('conflict requires explicit reload and simultaneous submit has one dispatcher', async () => {
  const operation = new ExactOperation<string>();
  let release!: () => void;
  let calls = 0;
  const first = operation.run(() => ({ key: 'two', payload: 'body' }), async () => {
    calls++; await new Promise<void>((resolve) => { release = resolve; }); throw { status: 409 };
  });
  await new Promise<void>((resolve) => setImmediate(resolve));
  await assert.rejects(operation.run(() => ({ key: 'replacement', payload: 'changed' }), async () => { calls++; }));
  release(); await assert.rejects(first);
  assert.equal(calls, 1);
  assert.equal(operation.conflict, true);
  await assert.rejects(operation.run(() => ({ key: 'three', payload: 'changed' }), async () => { calls++; }));
  operation.releaseConflict();
  await operation.run(() => ({ key: 'three', payload: 'updated' }), async () => { calls++; });
  assert.equal(calls, 2);
});
