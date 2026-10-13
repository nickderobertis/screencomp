import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdirSync, rmSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { test } from 'node:test';
import { root } from '../workflow-graph.mjs';
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
