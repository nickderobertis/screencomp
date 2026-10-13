import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { test } from 'node:test';
import { join } from 'node:path';
import { root, workflows, evaluate } from '../workflow-graph.mjs';
const productionRun = (needs, eventName = 'pull_request', manual = false) => {
  const step = workflows()['ci.yml'].jobs['macos-context'].steps.at(-1);
  const context = { needs, github: { event_name: eventName }, inputs: { 'macos-diagnostic': manual } };
  const env = Object.fromEntries(Object.entries(step.env).map(([key, expression]) => [key, String(evaluate(expression, context))]));
  return spawnSync('bash', ['-c', step.run], { cwd: root, encoding: 'utf8', env: { ...process.env, ...env, GITHUB_EVENT_NAME: eventName } });
};
const run = (needs, eventName = 'pull_request', manual = 'false') => spawnSync(process.execPath, [join(root, '.github/macos-result.mjs')], { encoding: 'utf8', env: { ...process.env, NEEDS_JSON: JSON.stringify(needs), GITHUB_EVENT_NAME: eventName, MANUAL_MACOS: manual } });
test('always carrier executes the real result command for failures, cancellation and authorized omissions', () => {
  const job = workflows()['ci.yml'].jobs['macos-context'];
  assert.equal(job.name, 'check (macos-latest)');
  assert.deepEqual(job.needs, ['policy', 'check-macos']);
  assert.equal(job.steps.at(-1).run, 'node .github/macos-result.mjs');
  for (const required of [true, false]) for (const policyResult of ['success', 'failure', 'cancelled', 'skipped', undefined]) for (const macResult of ['success', 'failure', 'cancelled', 'skipped', undefined]) {
    const needs = { policy: { result: policyResult, outputs: { macosRequired: String(required), isReleaseOnly: String(!required) } }, 'check-macos': { result: macResult } };
    assert.equal(evaluate(job.if, { needs }, [policyResult, macResult]), true);
    const got = productionRun(needs);
    const success = policyResult === 'success' && macResult === (required ? 'success' : 'skipped');
    assert.equal(got.status === 0, success, JSON.stringify(needs));
    if (success && !required) assert.match(got.stdout, /no Mac tests ran/);
  }
  const omitted = { policy: { result: 'success', outputs: { macosRequired: 'false', isReleaseOnly: 'false' } }, 'check-macos': { result: 'skipped' } };
  assert.notEqual(run(omitted).status, 0);
  for (const eventName of ['', 'unknown', undefined]) assert.notEqual(run(omitted, eventName).status, 0);
  assert.notEqual(run(omitted, 'push', 'yes').status, 0);
  assert.notEqual(run(omitted, 'push', 'true').status, 0);
  assert.equal(run(omitted, 'push').status, 0);
  assert.notEqual(run({}).status, 0);
  assert.notEqual(run({ policy: { result: 'success', outputs: {} } }).status, 0);
  assert.notEqual(run(omitted, 'workflow_dispatch', 'true').status, 0);
  omitted['check-macos'].result = 'success';
  assert.equal(run(omitted, 'workflow_dispatch', 'true').status, 0);
});

test('carrier production environment admits manual coverage only when opted in and successful', () => {
  for (const manual of [false, true]) for (const result of ['success', 'failure', 'cancelled', 'skipped', undefined]) {
    const needs = { policy: { result: 'success', outputs: { macosRequired: 'false', isReleaseOnly: 'false' } }, 'check-macos': { result } };
    const got = productionRun(needs, 'workflow_dispatch', manual);
    assert.equal(got.status === 0, result === (manual ? 'success' : 'skipped'));
  }
});
