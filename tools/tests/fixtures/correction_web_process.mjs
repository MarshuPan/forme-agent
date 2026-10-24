// Real authenticated Web, exact owner feedback and restart-safe receipt lookup.
import assert from 'node:assert/strict';
import { promises as fs } from 'node:fs';

async function readWorkbench(product, section, key) {
  for (let attempt = 0; attempt < 40; attempt++) {
    try {
      return await product.request('/api/v1/brain/workbench/query', { method: 'POST', key, body: {
        schema_version: 1, section, scope: null, offset: 0, limit: 100, snapshot: null,
      } });
    } catch (error) {
      // A concurrent tick can invalidate a read snapshot. Never retry writes here.
      if (!(error instanceof assert.AssertionError) || error.actual !== 409 || attempt === 39) throw error;
      await new Promise((resolve) => setTimeout(resolve, 100));
    }
  }
}

export async function exerciseWebCorrection(product, coreConfig) {
  const goal = 'goal:process-owner-feedback';
  const scope = coreConfig.brain.workspace;
  const base = '/api/v1/brain/workbench';
  await product.request('/api/v1/brain/learning/goals', { method: 'POST', key: 'learning:correction-goal', body: {
    schema_version: 1, command: 'learning:correction-goal', goal, knowledge: { schema_version: 1, scope,
      allow_questions: true, requirements: [{ schema_version: 1, target: { UserAttribute: 'temporary-interest' },
        reason: 'process fixture checks a scoped owner preference', impact: 'High', minimum_confidence: 0.8 }] },
  } });
  // The fixture controls its own core credential. Web cannot expose this tick.
  const credential = (await fs.readFile(coreConfig.transport.owner_credential_path, 'utf8')).trim();
  const tick = await fetch(`http://${product.coreAddress}/v1/runtime/tick`, {
    method: 'POST', signal: AbortSignal.timeout(30000), headers: { Authorization: `Bearer ${credential}`,
      Origin: `http://${product.coreAddress}`, 'X-Forme-Csrf': '1' },
  });
  assert.equal(tick.status, 200, 'fixture owner tick must use the real gateway');
  let ask;
  for (let attempt = 0; attempt < 60; attempt++) {
    const inbox = await readWorkbench(product, 'Inbox', 'correction:inbox');
    ask = inbox.items.find((item) => item.kind === 'AskToLearn' && item.learning?.goal === goal);
    if (ask) break;
    await new Promise((resolve) => setTimeout(resolve, 250));
  }
  assert.ok(ask, 'the real cognitive loop must generate the learning request');
  const answer = await product.request('/api/v1/brain/learning/answer', { method: 'POST', key: 'learning:correction-answer', body: {
    schema_version: 1, command: 'learning:correction-answer', reply: { schema_version: 1, proposal: ask.id, value: 'content:fixture-temporary-interest' },
  } });
  const view = await readWorkbench(product, 'Cognition', 'correction:cognition');
  const row = view.items.find((item) => item.id === answer.candidate && item.kind === 'Candidate');
  assert.ok(row);
  const command = { schema_version: 1, command: 'correction:process', run: row.run, kind: 'Candidate',
    id: row.id, event: row.event, snapshot: view.digest, statement: 'process-owner-feedback-7aa: a temporary interest is not a permanent preference' };
  await product.request(`${base}/correction`, { method: 'POST', key: 'wrong-key', body: command, expected: 400 });
  const receipt = await product.request(`${base}/correction`, { method: 'POST', key: command.command, body: command });
  assert.equal(receipt.status, 'Confirmed');
  const queried = await product.request(`${base}/corrections/query`, { method: 'POST', key: 'correction:receipt', body: {
    schema_version: 1, command: command.command,
  } });
  assert.deepEqual(queried.receipts, [receipt]);
  assert.deepEqual(await product.request(`${base}/correction`, { method: 'POST', key: command.command, body: command }), receipt);
  await product.request(`${base}/correction`, { method: 'POST', key: command.command, body: { ...command, statement: 'different' }, expected: 409 });
  assert.equal(JSON.stringify(queried).includes(command.statement), false);
  return { command, receipt };
}
