import { test } from 'node:test';
import assert from 'node:assert/strict';
import { CoreReadinessCancelled, isRetryableCoreError, waitForCoreReady } from '../core-readiness.ts';

function apiError(status: number) { return Object.assign(new Error(`status ${status}`), { status }); }

test('core readiness retries only transient availability failures with bounded backoff', async () => {
  const calls: number[] = [], delays: number[] = [];
  const result = await waitForCoreReady(async () => {
    calls.push(calls.length);
    if (calls.length < 3) throw apiError(503);
    return 'ready';
  }, { sleep: async (delay) => { delays.push(delay); } });
  assert.equal(result, 'ready');
  assert.equal(calls.length, 3);
  assert.deepEqual(delays, [500, 1000]);
});

test('core readiness stops on rate limiting and authentication failures', async () => {
  for (const status of [401, 403, 429]) {
    let calls = 0;
    await assert.rejects(waitForCoreReady(async () => { calls += 1; throw apiError(status); }, { sleep: async () => {} }), /status/);
    assert.equal(calls, 1);
    assert.equal(isRetryableCoreError(apiError(status)), false);
  }
});

test('core readiness has a finite retry window', async () => {
  let calls = 0;
  await assert.rejects(waitForCoreReady(async () => { calls += 1; throw apiError(503); }, { sleep: async () => {} }), /status/);
  assert.equal(calls, 6);
});

test('cancelling core readiness stops before the next probe', async () => {
  const controller = new AbortController();
  let calls = 0;
  await assert.rejects(waitForCoreReady(async () => { calls += 1; throw apiError(503); }, {
    signal: controller.signal,
    sleep: async () => { controller.abort(); },
  }), (error: unknown) => error instanceof CoreReadinessCancelled && error.name === 'AbortError');
  assert.equal(calls, 1);
});
