import assert from 'node:assert/strict';
import test from 'node:test';
import { inboxItemStatus } from '../workbench.ts';

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
