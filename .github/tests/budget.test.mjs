import assert from 'node:assert/strict';
import { spawnSync, execFileSync } from 'node:child_process';
import { mkdirSync, rmSync, writeFileSync, mkdtempSync, copyFileSync, symlinkSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { test } from 'node:test';
import { root, sourceDigest } from '../workflow-graph.mjs';
test('budget readers reject missing telemetry and stale source digests without rerunning scenarios', () => {
  const run = path => spawnSync(process.execPath, [join(root, '.github/macos-budget.mjs'), '--metric', 'jobs-per-run', '--telemetry', path], { encoding: 'utf8' });
  assert.notEqual(run(join(root, '.github/.telemetry/absent.json')).status, 0);
  const path = join(root, '.github/.telemetry/stale.json');
  // Independent fixture; the graph journey owns the real telemetry file.
  mkdirSync(join(root, '.github/.telemetry'), { recursive: true });
  try {
    writeFileSync(path, 'null');
    const invalidRoot = run(path);
    assert.notEqual(invalidRoot.status, 0); assert.match(invalidRoot.stderr, /invalid Mac policy telemetry root/);
    writeFileSync(path, JSON.stringify({ schemaVersion: 1, sourceDigest: 'stale' }));
    const stale = run(path);
    assert.notEqual(stale.status, 0); assert.match(stale.stderr, /stale Mac policy telemetry/);
  } finally { rmSync(path, { force: true }); }
});

test('telemetry reader rejects invalid metrics and nested rows at the recorded-data boundary', () => {
  const path = join(root, '.github/.telemetry/invalid.json');
  const baseline = { schemaVersion: 1, sourceDigest: sourceDigest(), releaseTargets: ['x86_64-apple-darwin'], jobsPerRun: 0, runsPerChange: 0, scenarios: [{ id: 'fixture', runs: [{ workflow: 'ci.yml', rows: [] }] }], lifecycles: [{ id: 'fixture', events: [], spendingRuns: [] }] };
  mkdirSync(join(root, '.github/.telemetry'), { recursive: true });
  try {
    for (const patch of [{ jobsPerRun: -1 }, { runsPerChange: 1.5 }, { jobsPerRun: '0' }, { jobsPerRun: 1 }, { releaseTargets: [null] }, { scenarios: [{ id: 'bad', runs: [null] }] }, { scenarios: [{ id: 'bad', runs: [{ workflow: 'ci.yml', rows: [{}] }] }] }, { lifecycles: [{ id: 'bad', events: [], spendingRuns: [{}] }] }, { scenarios: [{ id: 'bad', runs: [{ workflow: 'ci.yml', rows: [{ workflow: 'ci.yml', job: 'check-macos', displayName: 'check-macos', runner: 'macos-latest', event: 'pull_request', head: 'exact-head', enabled: true, macos: false, matrix: {}, targets: [], needs: [], tag: null }] }] }] }]) {
      writeFileSync(path, JSON.stringify({ ...baseline, ...patch }));
      const result = spawnSync(process.execPath, [join(root, '.github/macos-budget.mjs'), '--metric', 'jobs-per-run', '--telemetry', path], { encoding: 'utf8' });
      assert.notEqual(result.status, 0, JSON.stringify(patch)); assert.match(result.stderr, /invalid .*telemetry|inconsistent telemetry totals/);
    }
  } finally { rmSync(path, { force: true }); }
});

test('standalone budget output reports recorded measurements and rejects telemetry after a real source edit', () => {
  const dir = mkdtempSync(join(tmpdir(), 'screencomp-budget-source-'));
  try {
    mkdirSync(join(dir, '.github'));
    for (const file of ['.gitignore', '.github/.gitignore', '.github/package.json', '.github/workflow-graph.mjs', '.github/macos-budget.mjs']) copyFileSync(join(root, file), join(dir, file));
    symlinkSync(join(root, '.github/node_modules'), join(dir, '.github/node_modules'), 'junction');
    execFileSync('git', ['init', '-q'], { cwd: dir });
    execFileSync('git', ['add', '.gitignore', '.github/.gitignore', '.github/package.json', '.github/workflow-graph.mjs', '.github/macos-budget.mjs'], { cwd: dir });
    const digest = execFileSync(process.execPath, ['--input-type=module', '-e', "import { sourceDigest } from './.github/workflow-graph.mjs'; console.log(sourceDigest());"], { cwd: dir, encoding: 'utf8' }).trim();
    mkdirSync(join(dir, '.github/.telemetry'));
    const path = join(dir, '.github/.telemetry/policy.json');
    writeFileSync(path, JSON.stringify({ schemaVersion: 1, sourceDigest: digest, releaseTargets: ['x86_64-apple-darwin'], jobsPerRun: 0, runsPerChange: 0, scenarios: [{ id: 'fixture', runs: [{ workflow: 'ci.yml', rows: [] }] }], lifecycles: [{ id: 'fixture', events: [], spendingRuns: [] }] }));
    const env = { ...process.env }; delete env.ONEBUDGETSPEC_RESULT;
    const run = metric => spawnSync(process.execPath, [join(dir, '.github/macos-budget.mjs'), '--metric', metric, '--telemetry', path], { cwd: dir, env, encoding: 'utf8' });
    for (const metric of ['jobs-per-run', 'runs-per-change']) {
      const got = run(metric); assert.equal(got.status, 0, got.stderr);
      const output = JSON.parse(got.stdout); assert.equal(output.metric, metric); assert.equal(output.value, 0);
      assert.equal(output.detail.sourceDigest, digest); assert.deepEqual(output.detail.releaseTargets, ['x86_64-apple-darwin']);
      assert.equal(output.detail.scenarios[0].workflows[0].workflow, 'ci.yml');
    }
    writeFileSync(join(dir, '.github/package.json'), '{"private":true,"changed":true}');
    const stale = run('jobs-per-run'); assert.notEqual(stale.status, 0); assert.match(stale.stderr, /stale Mac policy telemetry/); assert.equal(stale.stdout, '');
  } finally { rmSync(dir, { recursive: true, force: true }); }
});
