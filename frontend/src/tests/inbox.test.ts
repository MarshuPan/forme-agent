import assert from 'node:assert/strict';
import test from 'node:test';
import { inboxItemStatus, isWorkbenchProposal, pendingProposalFor } from '../workbench.ts';

test('waiting inbox items preserve the typed pending reason', () => {
  const labels = {
    ApprovalWait: '等待 owner 处理审批',
    ToolInterrupt: '工具执行结果需要核实',
    Handoff: '等待协作结果',
    ResolutionUnknown: '运行结果待核实',
  };

  for (const [state, label] of Object.entries(labels)) {
    assert.equal(inboxItemStatus({ kind: 'Waiting', state }), label);
  }
  assert.equal(inboxItemStatus({ kind: 'Waiting', state: 'Unexpected' }), '运行待决状态待核实');
});

test('terminal failure labels do not turn terminal runs into waiting inbox items', () => {
  assert.equal(inboxItemStatus({ kind: 'Failure', state: 'Recorded' }), '已记录');
});

test('proposal recovery attaches only to the exact suggestion target', () => {
  const command = {
    schema_version: 1 as const,
    command: 'proposal:original',
    run: 'run:one',
    proposal: 'proposal:one',
    outcome: 'Defer' as const,
    defer_until: 2_000_000_000_000,
  };
  assert.equal(pendingProposalFor({ kind: 'Suggestion', run: 'run:one', id: 'proposal:one' }, [command]), command);
  assert.equal(pendingProposalFor({ kind: 'Suggestion', run: 'run:two', id: 'proposal:one' }, [command]), undefined);
  assert.equal(pendingProposalFor({ kind: 'Failure', run: 'run:one', id: 'proposal:one' }, [command]), undefined);
});

test('restored proposal commands reject mutations, invalid identities and unrepresentable dates', () => {
  const command = { schema_version: 1, command: 'proposal:key', run: 'run:one', proposal: 'proposal:one', outcome: 'Adopt', defer_until: null };
  assert.equal(isWorkbenchProposal(command), true);
  for (const changed of [
    { ...command, actor: 'Owner' }, { ...command, command: '' }, { ...command, run: 'run:\n' },
    { ...command, proposal: '字'.repeat(100) }, { ...command, outcome: 'unexpected' },
    { ...command, defer_until: 123 }, { ...command, outcome: 'Defer' },
    { ...command, outcome: 'Defer', defer_until: NaN }, { ...command, outcome: 'Defer', defer_until: Infinity },
    { ...command, outcome: 'Defer', defer_until: 9_007_199_254_740_991 },
  ]) assert.equal(isWorkbenchProposal(changed), false);
  assert.equal(isWorkbenchProposal({ ...command, outcome: 'Defer', defer_until: 2_000_000_000_000 }), true);
});
