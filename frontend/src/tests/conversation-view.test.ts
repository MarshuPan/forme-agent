import test from 'node:test';
import assert from 'node:assert/strict';
import { acceptedUserInput, mergeObservedRun } from '../conversation-view.ts';
import { emptyEvents, reduceEvents } from '../events.ts';
import type { BrainEvent, RunSummary } from '../types.ts';

const input = '<script>fixture-only</script>\n请保留原始输入。';
const accepted = (): BrainEvent => ({ schema_version: 1, event_id: 'event:accepted', run_id: 'run:one',
  stream_seq: 1, kind: 'RunAccepted', payload: { RunAccepted: { source: 'UserTurn', session_ref: 'session:one', input_ref: input } } });
const summary = (seq: number, status: RunSummary['status']): RunSummary => ({ schema_version: 1, run: 'run:one',
  session: 'session:one', workspace: 'workspace:one', source: 'UserTurn', last_stream_seq: seq, status, result: null });

test('history reconstructs the accepted owner text and preserves plain text verbatim', () => {
  const state = reduceEvents(emptyEvents('run:one'), [accepted()]);
  assert.equal(acceptedUserInput(state, 'session:one'), input);
  assert.equal(acceptedUserInput(reduceEvents(emptyEvents('run:one'), state.events), 'session:one'), input);
  assert.equal(acceptedUserInput(emptyEvents('run:one'), 'session:one'), null);
});

test('foreign sessions, gaps, duplicate acceptances and non-owner inputs do not become user messages', () => {
  const event = accepted(), state = reduceEvents(emptyEvents('run:one'), [event]);
  assert.throws(() => acceptedUserInput(state, 'session:other'));
  assert.throws(() => acceptedUserInput({ ...state, gap: true }, 'session:one'));
  assert.throws(() => acceptedUserInput({ ...state, run: 'run:other' }, 'session:one'));
  assert.throws(() => acceptedUserInput({ ...state, events: [event, { ...event, event_id: 'event:duplicate', stream_seq: 2 }] }, 'session:one'));
  const background = { ...event, payload: { RunAccepted: { source: 'ScheduledJob', session_ref: 'session:one', input_ref: 'system input' } } };
  assert.equal(acceptedUserInput({ ...state, events: [background] }, 'session:one'), null);
});

test('polling completion updates shared lists and late responses cannot regress the terminal status', () => {
  const other = { ...summary(4, 'Complete'), run: 'run:other' };
  const runs = [summary(3, 'Running'), other];
  const completed = mergeObservedRun(runs, summary(19, 'Complete'));
  assert.equal(completed[0].status, 'Complete');
  assert.equal(completed[1], other);
  assert.equal(mergeObservedRun(completed, summary(5, 'Running')), completed);
  assert.equal(mergeObservedRun(completed, summary(19, 'Complete')), completed);
  assert.deepEqual(mergeObservedRun([], summary(3, 'Waiting')), [summary(3, 'Waiting')]);
});
