import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createAuthInvalidationSync, type AuthInvalidationTransport } from '../auth-sync.ts';
import { ApiError, WorkbenchClient } from '../api/client.ts';

class FakeTransport implements AuthInvalidationTransport {
  readonly sent: unknown[] = [];
  private listener: ((data: unknown) => void) | null = null;
  private readonly peers: FakeTransport[];

  constructor(peers: FakeTransport[]) {
    this.peers = peers;
    peers.push(this);
  }

  send(message: { schema_version: 1; type: 'session-invalidated' }) {
    this.sent.push(message);
    for (const peer of this.peers) {
      if (peer !== this) peer.listener?.(message);
    }
  }

  subscribe(listener: (data: unknown) => void) {
    this.listener = listener;
    return () => { this.listener = null; };
  }

  close() { this.listener = null; }
  deliver(data: unknown) { this.listener?.(data); }
}

test('session invalidation crosses tabs without exposing session data', () => {
  const peers: FakeTransport[] = [];
  const senderTransport = new FakeTransport(peers);
  const receiverTransport = new FakeTransport(peers);
  let invalidations = 0;
  const sender = createAuthInvalidationSync(() => assert.fail('sender must not receive its own message'), () => senderTransport);
  const receiver = createAuthInvalidationSync(() => { invalidations++; }, () => receiverTransport);

  sender.notifyInvalidated();

  assert.equal(invalidations, 1);
  assert.deepEqual(senderTransport.sent, [{ schema_version: 1, type: 'session-invalidated' }]);
  sender.close();
  receiver.close();
});

test('malformed notifications are ignored and closed listeners stop receiving', () => {
  const peers: FakeTransport[] = [];
  const transport = new FakeTransport(peers);
  let invalidations = 0;
  const sync = createAuthInvalidationSync(() => { invalidations++; }, () => transport);

  transport.deliver({ schema_version: 1, type: 'session-invalidated', owner: 'owner:other' });
  transport.deliver({ schema_version: 2, type: 'session-invalidated' });
  assert.equal(invalidations, 0);

  sync.close();
  transport.deliver({ schema_version: 1, type: 'session-invalidated' });
  assert.equal(invalidations, 0);
});

test('unavailable cross-tab transport leaves local sign-out usable', () => {
  let invalidations = 0;
  const sync = createAuthInvalidationSync(() => { invalidations++; }, () => null);
  assert.doesNotThrow(() => sync.notifyInvalidated());
  sync.close();
  assert.equal(invalidations, 0);
});

test('peer invalidation fences an in-flight response from the previous session', async () => {
  const peers: FakeTransport[] = [];
  const senderTransport = new FakeTransport(peers);
  const receiverTransport = new FakeTransport(peers);
  const deferred: { resolve?: (response: Response) => void } = {};
  const client = new WorkbenchClient(() => new Promise((resolve) => { deferred.resolve = resolve; }));
  const sender = createAuthInvalidationSync(() => assert.fail('sender must not receive its own message'), () => senderTransport);
  const receiver = createAuthInvalidationSync(() => client.clear(), () => receiverTransport);
  const pending = client.session();

  sender.notifyInvalidated();
  const resolveResponse = deferred.resolve;
  assert.ok(resolveResponse);
  resolveResponse(Response.json({ schema_version: 1, session: {}, csrf: 'stale' }));

  await assert.rejects(pending, (error: unknown) => error instanceof ApiError && error.code === 'SessionChanged');
  sender.close();
  receiver.close();
});
