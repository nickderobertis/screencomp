import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdirSync, rmSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { test } from 'node:test';
import { root, sourceDigest } from '../workflow-graph.mjs';
test('budget readers reject missing telemetry and stale source digests without rerunning scenarios', () => {
  const run = path => spawnSync(process.execPath, [join(root, '.github/macos-budget.mjs'), '--metric', 'jobs-per-run', '--telemetry', path], { encoding: 'utf8' });
  assert.notEqual(run(join(root, '.github/.telemetry/absent.json')).status, 0);
  const path = join(root, '.github/.telemetry/stale.json');
  // Independent fixture; the graph journey owns the real telemetry file.
  mkdirSync(join(root, '.github/.telemetry'), { recursive: true });
  try {
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
    for (const patch of [{ jobsPerRun: -1 }, { runsPerChange: 1.5 }, { jobsPerRun: '0' }, { releaseTargets: [null] }, { scenarios: [{ id: 'bad', runs: [null] }] }, { scenarios: [{ id: 'bad', runs: [{ workflow: 'ci.yml', rows: [{}] }] }] }, { lifecycles: [{ id: 'bad', events: [], spendingRuns: [{}] }] }]) {
      writeFileSync(path, JSON.stringify({ ...baseline, ...patch }));
      const result = spawnSync(process.execPath, [join(root, '.github/macos-budget.mjs'), '--metric', 'jobs-per-run', '--telemetry', path], { encoding: 'utf8' });
      assert.notEqual(result.status, 0, JSON.stringify(patch)); assert.match(result.stderr, /invalid .*telemetry/);
    }
  } finally { rmSync(path, { force: true }); }
});
