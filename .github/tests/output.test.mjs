import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtempSync, writeFileSync, rmSync, readFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { root } from '../workflow-graph.mjs';
test('workflow output command prints exactly the validated classifier booleans and rejects malformed data', () => {
  const dir = mkdtempSync(join(tmpdir(), 'screencomp-output-')); const file = join(dir, 'policy.json');
  const run = raw => { writeFileSync(file, raw); return spawnSync(process.execPath, [join(root, '.github/macos-output.mjs'), file], { encoding: 'utf8' }); };
  try {
    for (const required of [true, false]) {
      const got = run(JSON.stringify({ macosRequired: required, isReleaseOnly: !required, reason: 'classified' }));
      assert.equal(got.status, 0); assert.equal(got.stdout, `macosRequired=${required}\nisReleaseOnly=${!required}\n`);
    }
    for (const raw of ['{', 'null', '{}', '{"macosRequired":"false","isReleaseOnly":true}', '{"macosRequired":true}']) assert.notEqual(run(raw).status, 0, raw);
  } finally { rmSync(dir, { recursive: true, force: true }); }
});

test('workflow output command validates its path argument before reading a file', () => {
  for (const args of [[], [''], ['policy.json', 'extra.json']]) {
    const got = spawnSync(process.execPath, [join(root, '.github/macos-output.mjs'), ...args], { encoding: 'utf8' });
    assert.notEqual(got.status, 0);
    assert.equal(got.stdout, '');
    assert.match(got.stderr, /usage: node .github\/macos-output\.mjs <policy-json-path>/);
  }
});

test('locked installer rejects a non-object package before invoking package managers', () => {
  const dir = mkdtempSync(join(tmpdir(), 'screencomp-package-'));
  try {
    writeFileSync(join(dir, 'install.mjs'), readFileSync(join(root, '.github/install.mjs')));
    for (const value of [null, [], false]) {
      writeFileSync(join(dir, 'package.json'), JSON.stringify(value));
      const got = spawnSync(process.execPath, [join(dir, 'install.mjs')], { encoding: 'utf8' });
      assert.notEqual(got.status, 0); assert.match(got.stderr, /requires a package object; fix .github\/package.json/);
    }
  } finally { rmSync(dir, { recursive: true, force: true }); }
});
