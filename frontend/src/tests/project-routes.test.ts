import { test } from 'node:test';
import assert from 'node:assert/strict';
import { nodeStatusLabel, projectRouteView, routeStatusLabel, verificationLabel } from '../project-routes.ts';

const route = {
  schema_version: 1, status: 'Running', spent: 2, remaining: 6, closed: false, checkpointed: true, conflicts: [],
  declaration: {
    schema_version: 1, route: 'route:one', project: 'project:one', pattern: 'pattern:plan-execute', confidence_millis: 720, budget_units: 8,
    nodes: [
      { schema_version: 1, id: 'research', role: 'Researcher', budget_units: 3 },
      { schema_version: 1, id: 'draft', role: 'Writer', budget_units: 5 },
    ],
    main: { schema_version: 1, id: 'main', role: 'Main', budget_units: 8 },
    dependencies: [{ schema_version: 1, from: 'research', to: 'draft', on_failure: 'Abort' }],
  },
  nodes: {
    research: { schema_version: 1, status: 'Waiting', attempts: 1, consumed: 1, reserved: 2, summary: '等待 owner 输入', verification: { Unverifiable: 'missing_source' }, evidence: ['event:1'] },
    draft: { schema_version: 1, status: 'Pending', attempts: 0, consumed: 0, reserved: 2, summary: '', verification: 'Fail', evidence: [] },
  },
};

test('project route view exposes DAG nodes, dependency blockers, progress and evidence', () => {
  const view = projectRouteView('route:fallback', route);
  assert.ok(view);
  assert.equal(view.reference, 'route:one');
  assert.equal(view.project, 'project:one');
  assert.equal(view.pattern, 'pattern:plan-execute');
  assert.equal(view.confidence, 720);
  assert.equal(view.budget, 8);
  assert.equal(view.nodes[0].budget, 3);
  assert.deepEqual(view.nodes[1].blockedBy, ['research']);
  assert.equal(view.nodes[0].status, 'Waiting');
  assert.deepEqual(view.nodes[0].evidence, ['event:1']);
  assert.equal(view.dependencies[0].onFailure, 'Abort');
  assert.equal(view.spent, 2);
});

test('malformed route data is omitted and user-facing labels distinguish unknown outcomes', () => {
  assert.equal(projectRouteView('route:bad', { status: 'Running' }), null);
  assert.equal(projectRouteView('route:future', { ...route, schema_version: 2 }), null);
  assert.equal(routeStatusLabel('OutcomeUnknown'), '结果待核实');
  assert.equal(nodeStatusLabel('OutcomeUnknown'), '结果待核实');
  assert.equal(verificationLabel('Unverifiable: missing_source'), '无法验证（missing_source）');
});

function pendingRoute() {
  const pending = structuredClone(route);
  pending.spent = 0;
  pending.checkpointed = false;
  for (const node of Object.values(pending.nodes)) {
    node.status = 'Pending';
    node.attempts = 0;
    node.consumed = 0;
    node.reserved = 0;
    node.summary = '';
    node.evidence = [];
  }
  return pending;
}

test('an open route is not advertised as executing before any node starts', () => {
  const pending = pendingRoute();
  assert.equal(routeStatusLabel('Running', projectRouteView('route:one', pending)), '已规划，待执行');
  assert.equal(routeStatusLabel('Running'), '路线开放');
  pending.nodes.research.attempts = 1;
  assert.equal(routeStatusLabel('Running', projectRouteView('route:one', pending)), '路线开放');
  pending.nodes.research.attempts = 0;
  pending.nodes.research.reserved = 1;
  assert.equal(routeStatusLabel('Running', projectRouteView('route:one', pending)), '路线开放');
  pending.nodes.research.reserved = 0;
  pending.spent = 1;
  assert.equal(routeStatusLabel('Running', projectRouteView('route:one', pending)), '路线开放');
});

test('node evidence distinguishes active execution, waiting and unresolved outcomes', () => {
  const active = structuredClone(route);
  assert.equal(routeStatusLabel('Running', projectRouteView('route:one', active)), '等待中');
  active.nodes.draft.status = 'Running';
  assert.equal(routeStatusLabel('Running', projectRouteView('route:one', active)), '执行中');
  active.nodes.research.status = 'OutcomeUnknown';
  assert.equal(routeStatusLabel('Running', projectRouteView('route:one', active)), '结果待核实');
  active.closed = true;
  assert.equal(routeStatusLabel('Running', projectRouteView('route:one', active)), '状态待核实');
});

test('missing node evidence stays unknown and terminal route status remains authoritative', () => {
  const pending = pendingRoute();
  const { status: _status, ...withoutStatus } = pending.nodes.research;
  const malformed = { ...pending, nodes: { ...pending.nodes, research: withoutStatus } };
  const view = projectRouteView('route:one', malformed);
  assert.ok(view);
  assert.equal(view.nodes[0].status, 'Unknown');
  assert.equal(routeStatusLabel('Running', view), '路线开放');
  const { attempts: _attempts, ...withoutAttempts } = pending.nodes.research;
  assert.equal(routeStatusLabel('Running', projectRouteView('route:one', {
    ...pending, nodes: { ...pending.nodes, research: withoutAttempts },
  })), '路线开放');
  assert.equal(routeStatusLabel('Completed', view), '已完成');
  assert.equal(routeStatusLabel('Failed', view), '失败');
  assert.equal(routeStatusLabel('Cancelled', view), '已取消');
});
