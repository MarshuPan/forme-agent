import test from 'node:test';
import assert from 'node:assert/strict';
import { WorkbenchClient } from '../api/client.ts';
import { chronologicalTurns, conversationList, readConversationTurn } from '../conversation-history.ts';
import type { BrainEvent, RunSummary, WorkbenchSessionRecord } from '../types.ts';

const session = (id: string): WorkbenchSessionRecord => ({ schema_version: 1, session: id, title: '持续讨论', workspace: null, archived: false, created_at: 1, updated_at: 1 });
const summary = (run: string, ownerSession = 'session:one'): RunSummary => ({ schema_version: 1, run,
  session: ownerSession, workspace: 'workspace:one', source: 'UserTurn', status: 'Complete', last_stream_seq: 2, result: null });
const accepted = (run: string, time: number, text: string): BrainEvent => ({ schema_version: 1, run_id: run, event_id: `event:${run}`,
  ts_unix_ms: time, stream_seq: 1, kind: 'RunAccepted', payload: { RunAccepted: { source: 'UserTurn', session_ref: 'session:one', input_ref: text } } });
const sse = (events: BrainEvent[]) => events.map((event) => `data: ${JSON.stringify(event)}\n\n`).join('');

test('sidebar counts conversations once, separates pending activity, and excludes archived and background runs', () => {
  const list = conversationList([session('session:one'), { ...session('session:two'), archived: true }], [
    summary('run:first'), { ...summary('run:second'), status: 'Waiting' }, summary('run:archived', 'session:two'),
    { ...summary('run:background'), source: 'ScheduledJob' },
  ]);
  assert.equal(list.length, 1);
  assert.equal(list[0].title, '持续讨论');
  assert.equal(list[0].turns, 2);
  assert.equal(list[0].activity, '等待处理');
});

test('two runs reload as one continuous transcript in acceptance-time order, independently of IDs and stream watermarks', async () => {
  const rows = new Map([['run:z-first', accepted('run:z-first', 100, '第一个问题')], ['run:a-followup', accepted('run:a-followup', 200, '继续上面的话题')]]);
  const calls: string[] = [];
  const api = new WorkbenchClient(async (path, options) => {
    assert.equal(options?.method, 'GET'); calls.push(String(path));
    const id = decodeURIComponent(String(path).split('/')[5]);
    if (String(path).includes('/events?')) return new Response(sse([rows.get(id)!]));
    if (String(path).endsWith('/answer')) return Response.json({ schema_version: 1, run: id, answer: `答复 ${id}` });
    return Response.json(summary(id));
  });
  const turns = chronologicalTurns(await Promise.all(['run:a-followup', 'run:z-first'].map((id) => readConversationTurn(api, 'session:one', id))));
  assert.deepEqual(turns.map((turn) => turn.input), ['第一个问题', '继续上面的话题']);
  assert.deepEqual(turns.map((turn) => turn.answer), ['答复 run:z-first', '答复 run:a-followup']);
  assert.equal(calls.length, 6);
});

test('history refuses a foreign run/session before reading its messages and refuses a foreign answer', async () => {
  let reads = 0;
  const wrongSession = new WorkbenchClient(async () => { reads++; return Response.json(summary('run:one', 'session:foreign')); });
  await assert.rejects(readConversationTurn(wrongSession, 'session:one', 'run:one'), /当前对话/);
  assert.equal(reads, 1);
  const wrongAnswer = new WorkbenchClient(async (path) => {
    if (String(path).includes('/events?')) return new Response(sse([accepted('run:one', 1, 'hello')]));
    if (String(path).endsWith('/answer')) return Response.json({ schema_version: 1, run: 'run:foreign', answer: 'foreign text' });
    return Response.json(summary('run:one'));
  });
  await assert.rejects(readConversationTurn(wrongAnswer, 'session:one', 'run:one'), /原消息/);
});

test('history resumes its own cursor, repairs a gap, and rejects history with no confirmed time', async () => {
  const event = accepted('run:one', 10, 'first');
  const cursors: number[] = [];
  let repaired = false, omitTime = false;
  const api = new WorkbenchClient(async (path) => {
    if (String(path).includes('/events?')) {
      const cursor = Number(String(path).split('after=')[1]); cursors.push(cursor);
      if (cursor === 1) { repaired = true; return new Response(sse([{ ...event, event_id: 'gap', stream_seq: 3, kind: 'RunComplete' }])); }
      return new Response(sse([{ ...event, ts_unix_ms: omitTime ? undefined : 10 }, ...(repaired ? [{ ...event, event_id: 'done', stream_seq: 2, kind: 'RunComplete' }] : [])]));
    }
    if (String(path).endsWith('/answer')) return Response.json({ schema_version: 1, run: 'run:one', answer: 'answer' });
    return Response.json(summary('run:one'));
  });
  const first = await readConversationTurn(api, 'session:one', 'run:one');
  const next = await readConversationTurn(api, 'session:one', 'run:one', first);
  assert.deepEqual(cursors, [0, 1, 0]); assert.equal(next.events.watermark, 2);
  omitTime = true;
  await assert.rejects(readConversationTurn(api, 'session:one', 'run:one'), /时间/);
});
