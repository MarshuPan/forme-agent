// Durable project supervision through actual Web, bridge and production core.
import assert from 'node:assert/strict';

export async function exerciseWebProjects(product, runtime) {
  const workspace = 'workspace:process-project';
  const charterPath = `/api/v1/brain/workspaces/${encodeURIComponent(workspace)}/charter`;
  const charter = { schema_version: 1, workspace, version: 0, goals: [],
    constraints: ['max-nodes:8'], prohibitions: [], done_contract: null,
    review_cadence: 60000, actor: 'Owner', digest: 'pending' };
  const prepared = await product.request(charterPath, { method: 'POST', key: 'charter:project-process', body: {
    schema_version: 1, command: 'charter:project-process', record: charter, expected_version: 0,
  } });
  await product.request(charterPath, { method: 'PUT', key: 'charter:project-process', body: {
    schema_version: 1, command: 'charter:project-process', change: prepared,
  } });
  const spec = { schema_version: 1, reference: 'project:process', workspace, goal: 'goal:process', done: 'done:process',
    summary: 'project supervision process fixture', signature: { schema_version: 1, partition: 'Whole',
      dependency: 'Independent', verification: 'SelfCheck', clarity: 'Clear', extent: 'Single',
      reversibility: 'LowRiskReversible', anchors: [] }, requirements: ['ActionSucceeded'], scope: workspace,
    capabilities: [], model: runtime.model_profiles[0], budget_units: 8, node_units: 2, cadence_ms: 60000, deadline: null };
  const projects = '/api/v1/brain/projects';
  await product.request(projects, { method: 'POST', key: 'project:register-process', body: {
    schema_version: 1, request: { operation: 'register', command: 'project:register-process', project: spec },
  } });
  const initial = await product.request(`${projects}/status`);
  assert.ok(initial.workspaces.includes(workspace));
  assert.ok(initial.workspaces.includes(runtime.workspace));
  const view = initial.projects.find((p) => p.project === spec.reference);
  assert.equal(view.review, 'Due');
  const pause = { schema_version: 1, command: 'project:pause-process', project: spec.reference,
    expected_project_digest: view.project_digest, expected_version: 0, enabled: false };
  const receipt = await product.request(`${projects}/supervision`, { method: 'POST', key: pause.command, body: pause });
  assert.equal(receipt.status, 'Confirmed');
  const signature = { ...spec.signature };
  await product.request(projects, { method: 'POST', key: 'route:explicit-paused', body: { schema_version: 1, request: {
    operation: 'plan', command: 'route:explicit-paused', project: spec.reference, signature, input_digest: view.project_digest,
  } } });
  const before = await product.request(projects);
  const revised = { ...spec, summary: 'revised subsequent segment only', cadence_ms: 120000 };
  await product.request(projects, { method: 'POST', key: 'project:revise-process', body: { schema_version: 1, request: {
    operation: 'revise', command: 'project:revise-process', project: revised, expected_digest: view.project_digest,
  } } });
  const after = await product.request(projects);
  assert.deepEqual(after.routes, before.routes, 'project edits must not mutate existing route declarations');
  await product.request(projects, { method: 'POST', key: 'project:revise-stale', expected: 409, body: { schema_version: 1, request: {
    operation: 'revise', command: 'project:revise-stale', project: spec, expected_digest: view.project_digest,
  } } });
  const verify = async () => {
    const snapshot = await product.request(`${projects}/status`);
    const current = snapshot.projects.find((p) => p.project === spec.reference);
    assert.equal(current.supervision.enabled, false);
    assert.equal(current.supervision.version, 1);
    assert.equal(current.review, 'Paused');
    assert.deepEqual((await product.request(projects)).projects[spec.reference], revised);
    assert.deepEqual(await product.request(`${projects}/supervision`, { method: 'POST', key: pause.command, body: pause }), receipt);
    assert.deepEqual((await product.request(projects)).routes, after.routes);
  };
  await verify();
  return { verify };
}
