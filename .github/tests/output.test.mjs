import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtempSync, writeFileSync, rmSync, readFileSync, mkdirSync, chmodSync } from 'node:fs';
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

test('locked installer selects matching Bun or pinned npm fallback and propagates failures', { skip: process.platform === 'win32' }, () => {
  const dir = mkdtempSync(join(tmpdir(), 'screencomp-install-routing-'));
  const bin = join(dir, 'bin'); const calls = join(dir, 'calls.json');
  const pkg = JSON.parse(readFileSync(join(root, '.github/package.json'), 'utf8'));
  const version = pkg.packageManager.slice(4);
  try {
    mkdirSync(bin);
    writeFileSync(join(dir, 'install.mjs'), readFileSync(join(root, '.github/install.mjs')));
    writeFileSync(join(dir, 'package.json'), JSON.stringify(pkg));
    const executable = name => {
      const path = join(bin, name);
      writeFileSync(path, `#!${process.execPath}\nimport fs from 'node:fs';\nconst args = process.argv.slice(2);\nif (args[0] === '--version') { console.log(process.env.BUN_VERSION); process.exit(0); }\nfs.writeFileSync(process.env.CALLS, JSON.stringify({ name: ${JSON.stringify(name)}, args }));\nif (process.env.INSTALL_FAIL === 'true') { console.error('registry transport failed'); process.exit(7); }\n`);
      chmodSync(path, 0o755);
    };
    executable('npm');
    for (const mode of ['matching', 'mismatched', 'missing']) for (const fail of [false, true]) {
      if (mode === 'missing') rmSync(join(bin, 'bun'), { force: true });
      else executable('bun');
      const got = spawnSync(process.execPath, [join(dir, 'install.mjs')], {
        cwd: dir, encoding: 'utf8', env: { ...process.env, PATH: bin, CALLS: calls, BUN_VERSION: mode === 'matching' ? version : '0.0.0', INSTALL_FAIL: String(fail) },
      });
      assert.deepEqual(JSON.parse(readFileSync(calls, 'utf8')), mode === 'matching'
        ? { name: 'bun', args: ['install', '--frozen-lockfile', '--cwd', '.github'] }
        : { name: 'npm', args: ['exec', '--yes', `--package=bun@${version}`, '--', 'bun', 'install', '--frozen-lockfile', '--cwd', '.github'] }, mode);
      if (fail) {
        assert.notEqual(got.status, 0, mode);
        assert.match(got.stderr, /registry transport failed/);
        assert.match(got.stderr, /locked GitHub tooling install failed/);
      } else {
        assert.equal(got.status, 0, got.stderr);
        assert.equal(got.stdout, ''); assert.equal(got.stderr, '');
      }
    }
  } finally { rmSync(dir, { recursive: true, force: true }); }
});
