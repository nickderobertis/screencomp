import assert from 'node:assert/strict';
import { execFileSync, spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { mkdtempSync, readFileSync, readdirSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { test } from 'node:test';
const root = resolve(import.meta.dirname, '../..');
const helper = join(root, 'scripts/release-artifact.mjs');
test('release production commands build, pack, checksum, install and run the real host artifact', { timeout: 240000 }, () => {
  const out = mkdtempSync(join(tmpdir(), 'screencomp-packaging-test-'));
  try {
    const target = /^host: (.*)$/m.exec(execFileSync('rustc', ['-vV'], { encoding: 'utf8' }))[1];
    const metadata = JSON.parse(execFileSync('cargo', ['metadata', '--no-deps', '--format-version', '1', '--locked'], { cwd: root, encoding: 'utf8' }));
    const version = metadata.packages.find(p => p.name === 'screencomp').version;
    const tag = `v${version}`;
    const invalid = spawnSync(process.execPath, [helper, target, 'v999.0.0', out], { cwd: root, encoding: 'utf8' });
    assert.notEqual(invalid.status, 0); assert.match(invalid.stderr, /tag does not match Cargo version/);
    assert.deepEqual(readdirSync(out), []);
    const built = spawnSync(process.execPath, [helper, target, tag, out], { cwd: root, encoding: 'utf8', timeout: 230000 });
    assert.equal(built.status, 0, built.stdout + built.stderr);
    const stem = `screencomp-${tag}-${target}`;
    const contract = JSON.parse(readFileSync(join(out, `${stem}.contract.json`), 'utf8'));
    const golden = JSON.parse(readFileSync(join(root, '.github/release-contract.json'), 'utf8'));
    assert.ok(golden.targets.includes(target));
    assert.equal(contract.schemaVersion, golden.schemaVersion);
    assert.equal(contract.tag, tag); assert.equal(contract.version, version); assert.equal(contract.target, target);
    const expectedContents = target.includes('windows') ? golden.contents.map(f => f === 'screencomp' ? golden.windowsBinary : f) : golden.contents;
    assert.deepEqual(contract.contents, expectedContents);
    assert.equal(contract.binarySha256, contract.installedSha256);
    const archive = join(out, contract.archive);
    const sum = createHash('sha256').update(readFileSync(archive)).digest('hex');
    assert.equal(readFileSync(join(out, contract.checksum), 'utf8'), `${sum}  ${contract.archive}\n`);
    const contents = execFileSync('tar', ['-tf', archive], { encoding: 'utf8' }).trim().split(/\r?\n/);
    assert.deepEqual(contents.sort(), [...expectedContents].sort());
    assert.match(built.stdout, /changed/);
    assert.match(built.stderr, /installed screencomp/);
  } finally { rmSync(out, { recursive: true, force: true }); }
});
