import { test } from 'node:test';
import assert from 'node:assert/strict';
import { WorkbenchClient } from '../api/client.ts';
import { canCorrect, correctionCommand, parseCorrectionReceipt, parseCorrectionsView } from '../correction.ts';
import type { WorkbenchItem } from '../workbench.ts';

const digest = 'sha256:' + 'a'.repeat(64);
const item: WorkbenchItem = { schema_version: 1, id: 'candidate:source', kind: 'Candidate', run: 'run:source',
  event: 'event:source', stream_seq: 3, at: 1, scope: 'workspace:one', state: 'Candidate', summary: 'old observation',
  content: null, tier: 'working', confidence: 0.4, evidence: [], evidence_count: 0, derived: [], derived_count: 0,
  reviews: ['Reject', 'Downgrade'], learning: null };
const command = correctionCommand(item, digest, '纠正内容 <script>只作为文本</script>', 'correction:test');
const receipt = { schema_version: 1, command: command.command, binding: digest, status: 'Confirmed',
  event: `owner-correction:${digest}:receipt`, candidate: `owner-correction-candidate:${digest}` };

test('only live scoped mutable cognition accepts feedback and UTF-8 limits match core', () => {
  assert.equal(canCorrect(item), true);
  for (const changed of [{ kind: 'Memory' }, { kind: 'Reevaluation' }, { state: 'ReviewUnknown' }, { state: 'Downgraded' },
    { state: 'Rejected' }, { tier: 'fixed' }, { tier: 'constitutional' }, { tier: null }, { scope: null }]) {
    const row = { ...item, ...changed } as WorkbenchItem;
    assert.equal(canCorrect(row), false); assert.throws(() => correctionCommand(row, digest, 'feedback', 'key'));
  }
  assert.equal(canCorrect({ ...item, kind: 'Stable', state: 'Active' }), true);
  assert.equal(canCorrect({ ...item, kind: 'Stable', state: 'Inactive' }), false);
  for (const text of [' ', '中'.repeat(2731), 'bad\0text', 'bad\u0085text']) {
    assert.throws(() => correctionCommand(item, digest, text, 'key'));
  }
  assert.equal(correctionCommand(item, digest, '中'.repeat(2730), 'key').statement.length, 2730);
});

test('correction uses only exact source snapshot and text, never actor, authority or target fields', async () => {
  let count = 0;
  const api = new WorkbenchClient(async (url, request) => {
    count++; assert.equal(url, '/api/v1/brain/workbench/correction');
    assert.equal(request?.credentials, 'same-origin'); assert.equal(request?.cache, 'no-store');
    assert.equal(new Headers(request?.headers).get('Idempotency-Key'), command.command);
    assert.deepEqual(JSON.parse(String(request?.body)), command);
    return Response.json(receipt);
  });
  assert.deepEqual(await api.correctCognition(command), receipt); assert.equal(count, 1);
});

test('unknown outcome performs no automatic retry and queries only the original command', async () => {
  const calls: string[] = [];
  const api = new WorkbenchClient(async (url, request) => {
    calls.push(String(url));
    if (String(url).endsWith('/correction')) throw new Error('fixture disconnect');
    assert.deepEqual(JSON.parse(String(request?.body)), { schema_version: 1, command: command.command });
    return Response.json({ schema_version: 1, receipts: [receipt], truncated: false });
  });
  await assert.rejects(api.correctCognition(command), /连接中断/);
  assert.equal((await api.corrections(command.command)).receipts.length, 1);
  assert.deepEqual(calls, ['/api/v1/brain/workbench/correction', '/api/v1/brain/workbench/corrections/query']);
});

test('receipts reject wrong command, schema, outcome, mismatched source refs and unbounded histories', () => {
  for (const changed of [{ command: 'other' }, { schema_version: 2 }, { status: 'Unknown' }, { binding: 'not-a-digest' },
    { candidate: 'candidate:other' }, { event: 'event:other' }, { statement: 'must not be returned' }]) {
    assert.throws(() => parseCorrectionReceipt({ ...receipt, ...changed }, command.command));
  }
  const view = { schema_version: 1, receipts: [receipt], truncated: false };
  assert.deepEqual(parseCorrectionsView(view, command.command), view);
  assert.throws(() => parseCorrectionsView({ ...view, receipts: [receipt, receipt] }));
  assert.throws(() => parseCorrectionsView({ ...view, receipts: Array.from({ length: 101 }, () => receipt) }));
  assert.throws(() => parseCorrectionsView({ ...view, truncated: 'no' }));
});

test('refresh lookup is read-only and does not include any correction text', async () => {
  const api = new WorkbenchClient(async (url, request) => {
    assert.equal(url, '/api/v1/brain/workbench/corrections/query');
    assert.deepEqual(JSON.parse(String(request?.body)), { schema_version: 1, command: null });
    return Response.json({ schema_version: 1, receipts: [receipt], truncated: false });
  });
  assert.equal((await api.corrections()).receipts[0].command, command.command);
});

test('logout while receiving feedback or history rejects stale owner data', async () => {
  for (const history of [true, false]) {
    let complete: ((value: Response) => void) | undefined;
    const api = new WorkbenchClient(() => new Promise<Response>((resolve) => { complete = resolve; }));
    const pending = history ? api.corrections(command.command) : api.correctCognition(command);
    api.clear(); complete!(Response.json(history ? { schema_version: 1, receipts: [receipt], truncated: false } : receipt));
    await assert.rejects(pending, /登录状态已变化/);
  }
});
