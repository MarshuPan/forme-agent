import { test } from 'node:test';
import assert from 'node:assert/strict';
import { emptyEvents, parseEvents, readDelay, reduceEvents } from '../events.ts';
import type { BrainEvent } from '../types.ts';

function event(seq: number, id = `event:${seq}`, run = 'run:one'): BrainEvent {
  return { schema_version: 1, event_id: id, run_id: run, stream_seq: seq, kind: 'RunAccepted', payload: { RunAccepted: { input_ref: 'A question' } } };
}
test('event reconnection deduplicates by authoritative run sequence and identity', () => {
  const state = reduceEvents(emptyEvents('run:one'), [event(1), event(2)]);
  assert.deepEqual(reduceEvents(state, [event(1), event(2)]), state);
  const next = reduceEvents(state, [event(2), event(3)]);
  assert.equal(next.watermark, 3); assert.equal(next.events.length, 3);
  assert.throws(() => reduceEvents(state, [event(2, 'event:conflict')]));
  assert.throws(() => reduceEvents(state, [event(3, 'event:3', 'run:other')]));
  assert.throws(() => reduceEvents(state, [event(3, 'event:1')]));
});
test('a gap leaves the original snapshot unchanged for a full resynchronization', () => {
  const previous = reduceEvents(emptyEvents('run:one'), [event(1)]);
  const result = reduceEvents(previous, [event(3)]);
  assert.equal(result.gap, true); assert.equal(result.watermark, 1); assert.equal(result.events.length, 1);
  const recovered = reduceEvents(emptyEvents('run:one'), [event(1), event(2), event(3)]);
  assert.equal(recovered.gap, false); assert.equal(recovered.watermark, 3);
});
test('SSE parsing handles CRLF and heartbeats without running content', () => {
  const malicious = event(1); malicious.payload = { RunAccepted: { input_ref: '<script>globalThis.executed=true</script>' } };
  const text = `: heartbeat\r\n\r\nid: run:one|1\r\nevent: forme-event\r\ndata: ${JSON.stringify(malicious)}\r\n\r\n`;
  assert.deepEqual(parseEvents(text), [malicious]);
  assert.throws(() => parseEvents('data: {"schema_version":2}\n\n'));
  assert.equal((globalThis as Record<string, unknown>).executed, undefined);
});
test('read recovery respects server throttling and bounded exponential backoff', () => {
  assert.equal(readDelay({ retryAfterMs: 60_000 }, 0, () => .5), 60_000);
  assert.equal(readDelay({}, 0, () => .5), 1000);
  assert.equal(readDelay({}, 9, () => .5), 30_000);
});
