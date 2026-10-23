import assert from 'node:assert/strict';
import test from 'node:test';
import { runStatusMessage } from '../run-status.ts';
import type { RunSummary } from '../types.ts';

function summary(status: RunSummary['status'], result: RunSummary['result'] = null): RunSummary {
  return {
    schema_version: 1,
    run: 'run:test',
    session: 'session:test',
    workspace: null,
    source: 'UserTurn',
    status,
    last_stream_seq: 1,
    result,
  };
}

test('unknown model-call outcome is disclosed without implying retry or success', () => {
  const message = runStatusMessage(summary('Failed', {
    schema_version: 1,
    status: 'Failed',
    stop_reason: 'model_call_outcome_unknown_after_restart',
    outputs: [],
    evidence_refs: [],
  }));
  assert.match(message, /回复和费用均无法确认/);
  assert.match(message, /没有自动重试/);
});

test('ordinary waiting and failed runs keep their existing descriptions', () => {
  assert.match(runStatusMessage(summary('Waiting')), /等待输入/);
  assert.match(runStatusMessage(summary('Failed')), /没有可展示的最终回答/);
});
