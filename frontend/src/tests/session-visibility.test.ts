import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createSessionVisibilityGuard } from '../session-visibility.ts';

function fixture() {
  const checks: { resolve(): void; reject(error: Error): void }[] = [];
  const state = { hidden: false, concealed: false, reveals: 0, invalidations: 0 };
  const guard = createSessionVisibilityGuard({
    verify: () => new Promise<void>((resolve, reject) => { checks.push({ resolve, reject }); }),
    hidden: () => state.hidden,
    conceal: () => { state.concealed = true; },
    reveal: () => { state.concealed = false; state.reveals++; },
    invalidated: () => { state.invalidations++; },
  });
  return { checks, state, guard };
}

const flush = async () => { for (let step = 0; step < 12; step++) await Promise.resolve(); };

test('restoring during an older session check waits for a fresh check and coalesces duplicate events', async () => {
  const { checks, state, guard } = fixture();
  const periodic = guard.check();
  await flush();
  assert.equal(checks.length, 1);
  guard.hide();
  const restored = guard.show();
  const repeated = guard.show();
  assert.equal(state.concealed, true);

  checks[0].resolve();
  await periodic;
  await flush();
  assert.equal(state.reveals, 0);
  assert.equal(state.concealed, true);
  assert.equal(checks.length, 2);

  checks[1].resolve();
  await Promise.all([restored, repeated]);
  assert.equal(state.reveals, 1);
  assert.equal(state.concealed, false);
  assert.equal(state.invalidations, 0);
  guard.close();
});

test('a current failed check invalidates without revealing the cached workbench', async () => {
  const { checks, state, guard } = fixture();
  const restored = guard.show();
  await flush();
  checks[0].reject(new Error('session is no longer authenticated'));
  await restored;
  assert.equal(state.concealed, true);
  assert.equal(state.invalidations, 1);
  assert.equal(state.reveals, 0);
  guard.close();
  assert.equal(state.concealed, false);
});

test('an old failed request cannot clear or reveal the restored session', async () => {
  const { checks, state, guard } = fixture();
  const old = guard.check();
  await flush();
  guard.hide();
  const restored = guard.show();
  checks[0].reject(new Error('old connection interrupted'));
  await old;
  await flush();
  assert.equal(state.invalidations, 0);
  assert.equal(state.reveals, 0);
  assert.equal(checks.length, 2);
  checks[1].resolve();
  await restored;
  assert.equal(state.reveals, 1);
  guard.close();
});

test('a page hidden again before verification completes remains concealed', async () => {
  const { checks, state, guard } = fixture();
  const first = guard.show();
  await flush();
  state.hidden = true;
  guard.hide();
  checks[0].resolve();
  await first;
  await guard.check();
  assert.equal(checks.length, 1);
  assert.equal(state.concealed, true);
  assert.equal(state.reveals, 0);

  state.hidden = false;
  const restored = guard.show();
  await flush();
  checks[1].resolve();
  await restored;
  assert.equal(state.reveals, 1);
  guard.close();
});

test('closing the guard fences in-flight callbacks and does not issue a queued check', async () => {
  const { checks, state, guard } = fixture();
  const old = guard.check();
  await flush();
  guard.hide();
  const restored = guard.show();
  guard.close();
  const revealsAfterClose = state.reveals;
  checks[0].reject(new Error('connection interrupted after unmount'));
  await Promise.all([old, restored]);
  await guard.show();
  await guard.check();
  assert.equal(checks.length, 1);
  assert.equal(state.invalidations, 0);
  assert.equal(state.reveals, revealsAfterClose);
});

test('closing before a scheduled check starts does not send a session request', async () => {
  const { checks, state, guard } = fixture();
  const restored = guard.show();
  guard.close();
  await restored;
  assert.equal(checks.length, 0);
  assert.equal(state.invalidations, 0);
  assert.equal(state.reveals, 1);
});

test('hiding before a scheduled check starts skips it and verifies on the next restore', async () => {
  const { checks, state, guard } = fixture();
  const first = guard.show();
  state.hidden = true;
  guard.hide();
  await first;
  assert.equal(checks.length, 0);
  assert.equal(state.concealed, true);
  assert.equal(state.reveals, 0);

  state.hidden = false;
  const restored = guard.show();
  await flush();
  assert.equal(checks.length, 1);
  checks[0].resolve();
  await restored;
  assert.equal(state.reveals, 1);
  guard.close();
});
