import { test } from 'node:test';
import assert from 'node:assert/strict';
import { ApiError, WorkbenchClient } from '../api/client.ts';
import { charterRulesError } from '../workspace-charter.ts';
import type { WorkspaceCharterRecord, ProjectCommand, WorkspaceProject } from '../types.ts';

const record: WorkspaceCharterRecord = {
  schema_version: 1, workspace: 'workspace:demo', version: 3, goals: [],
  constraints: ['read-only'], prohibitions: [], done_contract: null,
  review_cadence: 60000, actor: 'Owner', digest: `sha256:${'a'.repeat(64)}`,
};
const prepared = () => ({ charter: { ...structuredClone(record), version: 4, digest: `sha256:${'b'.repeat(64)}` },
  expected_version: 3, committed_version: 4 });

test('a real-shaped nested charter response is committed with the original command and read back', async () => {
  const calls: { method: string; body: unknown; key: string | null }[] = [];
  let committed: unknown = null;
  const api = new WorkbenchClient(async (_route, init) => {
    const body = init?.body ? JSON.parse(String(init.body)) : undefined;
    calls.push({ method: init!.method!, body, key: new Headers(init?.headers).get('Idempotency-Key') });
    if (init?.method === 'GET') return Response.json(committed);
    if (init?.method === 'POST') return Response.json(prepared());
    committed = body.change.charter;
    return Response.json({ schema_version: 1, event: 'event:charter' });
  });
  const change = await api.prepareCharter(record.workspace, record, 3, 'charter:one');
  await api.updateCharter(record.workspace, change, 'charter:one');
  assert.deepEqual(await api.charter(record.workspace), prepared().charter);
  assert.deepEqual(calls.map((call) => call.method), ['POST', 'PUT', 'GET']);
  assert.deepEqual(calls.slice(0, 2).map((call) => call.key), ['charter:one', 'charter:one']);
});

test('preparation rejects mismatched content, authority, versions and malformed responses before PUT', async () => {
  const original = prepared();
  const responses = [null, { schema_version: 1, event: 'not-a-preparation' }, { ...original, schema_version: 2 },
    { ...original, expected_version: 2 }, { ...original, committed_version: 5 },
    ...[{ schema_version: 2 }, { version: 5 }, { workspace: 'workspace:other' }, { actor: 'Model' },
      { constraints: ['allow-capability:all'] }, { goals: ['goal:replacement'] },
      { prohibitions: ['read-only'] }, { done_contract: 'done:replacement' }, { review_cadence: 1 },
      { digest: 'sha256:invalid' }].map((patch) => ({ ...original, charter: { ...original.charter, ...patch } }))];
  for (const response of responses) {
    const methods: string[] = [];
    const api = new WorkbenchClient(async (_route, init) => {
      methods.push(init!.method!); return Response.json(response);
    });
    await assert.rejects(async () => {
      const change = await api.prepareCharter(record.workspace, record, 3, 'charter:bound');
      await api.updateCharter(record.workspace, change, 'charter:bound');
    });
    assert.deepEqual(methods, ['POST']);
  }
});

test('a delayed nested response cannot cross a logout or session generation', async () => {
  let release!: () => void, decoding!: () => void;
  const started = new Promise<void>((resolve) => { decoding = resolve; });
  const hold = new Promise<void>((resolve) => { release = resolve; });
  const api = new WorkbenchClient(async () => {
    const response = Response.json(prepared());
    response.json = async () => { decoding(); await hold; return prepared(); };
    return response;
  });
  const request = api.prepareCharter(record.workspace, record, 3, 'charter:session-bound');
  await started; api.clear(); release();
  await assert.rejects(request, (problem: unknown) => problem instanceof ApiError && problem.code === 'SessionChanged');
});

test('unversioned project receipts are parsed by operation; ordinary views still reject an unknown schema', async () => {
  const project: WorkspaceProject = {
    schema_version: 1, reference: 'project:one', workspace: record.workspace, goal: 'goal:one', done: 'done:one',
    summary: 'Local project response fixture', signature: { schema_version: 1, partition: 'Whole', dependency: 'Independent',
      verification: 'SelfCheck', clarity: 'Clear', extent: 'Single', reversibility: 'LowRiskReversible', anchors: [] },
    requirements: ['ActionSucceeded'], scope: record.workspace, capabilities: [], model: 'model:fixture',
    budget_units: 8, node_units: 2, cadence_ms: 60000, deadline: null,
  };
  const commands: ProjectCommand[] = [
    { operation: 'cancel', command: 'cancel:one', route: 'route:one' },
    { operation: 'revise', command: 'revise:one', project, expected_digest: 'sha256:fixture' },
  ];
  for (const [index, command] of commands.entries()) {
    const api = new WorkbenchClient(async () => Response.json(index === 0 ? { cancelled: true } : { event: 'event:one' }));
    await api.project(command, 'project:one');
    await assert.rejects(api.project(commands[1 - index], 'project:other'));
    const unsupported = new WorkbenchClient(async () => Response.json({ schema_version: 2, event: 'event:one', cancelled: true }));
    await assert.rejects(unsupported.project(command, 'project:unsupported'));
  }
  const bad = new WorkbenchClient(async () => Response.json({ schema_version: 2 }));
  await assert.rejects(bad.projectStatus());
  await assert.rejects(bad.project({ operation: 'execute', route: 'route:one' }, 'execute:one'));
});

test('owner guidance distinguishes natural language from executable constraints without broadening scope', () => {
  assert.match(charterRulesError(record.workspace, ['仅使用当前资料'], []), /约束第 1 行.*可执行规则/);
  assert.match(charterRulesError(record.workspace, [], ['require-review']), /禁止规则第 1 行/);
  assert.match(charterRulesError(record.workspace, [], []), /至少/);
  assert.match(charterRulesError(record.workspace, ['scope:workspace:demolition'], []), /范围/);
  assert.match(charterRulesError(record.workspace, ['max-nodes:33'], []), /1–32/);
  assert.match(charterRulesError(record.workspace, ['max-nodes:2', 'max-nodes:3'], []), /只能设置一次/);
  assert.match(charterRulesError(record.workspace, ['deny-capability:'], []), /非空/);
  assert.match(charterRulesError(record.workspace, ['deny-capability:' + '界'.repeat(170)], []), /512 字节/);
  assert.match(charterRulesError(record.workspace, Array(65).fill('read-only'), []), /64 项/);
  assert.equal(charterRulesError(record.workspace,
    ['read-only', 'require-review', 'max-nodes:8', 'scope:workspace:demo/files', 'allow-capability:file:read'],
    ['deny-capability:file:write']), '');
});
