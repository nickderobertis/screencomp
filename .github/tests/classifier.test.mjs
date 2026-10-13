import assert from 'node:assert/strict';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { test } from 'node:test';
import { history, prEvent } from './history.mjs';
test('real git classifier admits only same-repository semantic release changes', () => {
  const h = history();
  try {
    for (const author of ['nickderobertis', 'github-actions[bot]']) for (const fork of [false, true]) for (const branch of ['fix/ordinary', 'release-plz-2026']) for (const kind of Object.keys(h.heads)) {
      const event = prEvent({ author, fork, branch, base: h.base, head: h.heads[kind] });
      const got = h.classify(event, h.heads[kind]);
      const release = !fork && branch.startsWith('release-plz-') && kind === 'version-only';
      assert.equal(got.isReleaseOnly, release, `${author} ${fork} ${branch} ${kind}`);
      assert.equal(got.macosRequired, !release);
      assert.equal(typeof got.reason, 'string');
    }
    for (const [raw, baseRef, head] of [['{', h.base, h.heads['version-only']], ['null', h.base, h.heads.source], [undefined, 'missing-ref', h.heads['version-only']], [undefined, h.base, 'missing-ref']]) {
      const got = h.classify(prEvent({ branch: 'release-plz-x' }), head, { raw, baseRef });
      assert.equal(got.macosRequired, true); assert.equal(got.isReleaseOnly, false);
    }
    for (const raw of ['{', 'null', '{}', '{"release":[]}', '{"ref":"refs/heads/main","before":"unknown"}']) assert.equal(h.classify({}, h.heads.source, { raw, omitEventName: true }).macosRequired, true);
    assert.equal(h.classify({ ref: 'refs/heads/main', before: h.base }, h.heads.source, { omitEventName: true }).macosRequired, false);
    assert.equal(h.classify({ release: { tag_name: 'v0.4.10' } }, h.heads.source, { omitEventName: true }).macosRequired, false);
    for (const eventName of ['', 'unknown']) assert.equal(h.classify(prEvent({ branch: 'release-plz-x' }), h.heads['version-only'], { eventName }).macosRequired, true);
    for (const eventName of ['push', 'release', 'schedule', 'workflow_run', 'repository_dispatch', 'workflow_dispatch']) assert.equal(h.classify({}, h.heads.source, { eventName, raw: '{' }).macosRequired, false);
    for (const [name, content] of [
      ['malformed', '[package]\nname = "screencomp"\nversion = "0.4.10"\n[broken'],
      ['nested-version', '[package]\nname = "screencomp"\nversion = "0.4.10"\nmetadata = { version = "1.0.0", quantity = 1, ratio = inf, date = 2026-01-01 }\n[dependencies]\nserde = "1"\n'],
    ]) {
      h.git('checkout', '-q', '-B', name, h.base);
      writeFileSync(join(h.dir, 'Cargo.toml'), content);
      h.git('add', '.'); h.git('commit', '-qm', name);
      assert.equal(h.classify(prEvent({ branch: 'release-plz-x' }), h.git('rev-parse', 'HEAD')).macosRequired, true);
    }
    for (const [name, change] of [
      ['dependency-lock', text => text.replace('version = "1.0.0"', 'version = "2.0.0"')],
      ['checksum-lock', text => text.replace('checksum = "abc"', 'checksum = "def"')],
      ['source-lock', text => text.replace('source = "registry"', 'source = "other"')],
    ]) {
      h.git('checkout', '-q', '-B', name, h.base);
      const path = join(h.dir, 'Cargo.lock');
      writeFileSync(path, change(readFileSync(path, 'utf8')));
      h.git('add', '.'); h.git('commit', '-qm', name);
      assert.equal(h.classify(prEvent({ branch: 'release-plz-x' }), h.git('rev-parse', 'HEAD')).macosRequired, true);
    }
    const malformedRepo = prEvent({ branch: 'release-plz-x' });
    malformedRepo.repository.full_name = 42; malformedRepo.pull_request.head.repo.full_name = 42; malformedRepo.pull_request.base.repo.full_name = 42;
    assert.equal(h.classify(malformedRepo, h.heads['version-only']).macosRequired, true);
    // A source edit on an otherwise valid release branch cannot hide behind a version bump.
    h.git('checkout', '-q', '-B', 'mixed', h.heads['version-only']);
    writeFileSync(join(h.dir, 'src/lib.rs'), 'pub fn mixed() {}');
    h.git('add', '.'); h.git('commit', '-qm', 'mixed');
    assert.equal(h.classify(prEvent({ branch: 'release-plz-x' }), h.git('rev-parse', 'HEAD')).macosRequired, true);
  } finally { h.clean(); }
});

test('semantic manifest variants and non-modification diffs are driven through generated histories', () => {
  const h = history();
  try {
    const event = prEvent({ branch: 'release-plz-x' });
    assert.equal(h.classify(event, h.base).macosRequired, true);
    for (const kind of ['added', 'deleted', 'renamed']) {
      h.git('checkout', '-q', '-B', kind, h.base);
      if (kind === 'added') writeFileSync(join(h.dir, 'new.txt'), 'new content');
      if (kind === 'deleted') h.git('rm', 'CHANGELOG.md');
      if (kind === 'renamed') h.git('mv', 'CHANGELOG.md', 'OTHER.md');
      h.git('add', '.'); h.git('commit', '-qm', kind);
      assert.equal(h.classify(event, h.git('rev-parse', 'HEAD')).macosRequired, true);
    }
    h.git('checkout', '-q', '-B', 'manifest-contracts', h.base);
    const manifest = readFileSync(join(h.dir, 'Cargo.toml'), 'utf8').replace('[dependencies]', 'metadata = { version = "1.0.0", quantity = 1, ratio = inf, date = 2026-01-01 }\n[dependencies]') + '[workspace]\nmembers = [".", "e2e"]\n[workspace.package]\nversion = "0.4.9"\nrust-version = "1.89"\n';
    writeFileSync(join(h.dir, 'Cargo.toml'), manifest);
    mkdirSync(join(h.dir, 'e2e'));
    const e2e = '[package]\nname = "screencomp-e2e"\nversion = "0.4.9"\npublish = false\n';
    writeFileSync(join(h.dir, 'e2e/Cargo.toml'), e2e);
    const pkg = { name: 'screencomp', version: '0.4.9', dependencies: { tool: '1' } };
    writeFileSync(join(h.dir, 'package.json'), JSON.stringify(pkg));
    const workspaceLock = readFileSync(join(h.dir, 'Cargo.lock'), 'utf8') + '[[package]]\nname = "screencomp-e2e"\nversion = "0.4.9"\n';
    writeFileSync(join(h.dir, 'Cargo.lock'), workspaceLock);
    h.git('add', '.'); h.git('commit', '-qm', 'manifest baseline'); const base = h.git('rev-parse', 'HEAD');
    const cases = [
      ['workspace-version', 'Cargo.toml', manifest.replace('[workspace.package]\nversion = "0.4.9"', '[workspace.package]\nversion = "0.4.10"'), true],
      ['numeric-type-content', 'Cargo.toml', manifest.replace('quantity = 1', 'quantity = 1.0'), false],
      ['nonfinite-content', 'Cargo.toml', manifest.replace('ratio = inf', 'ratio = nan'), false],
      ['date-type-content', 'Cargo.toml', manifest.replace('date = 2026-01-01', 'date = { tomlDate = "2026-01-01" }'), false],
      ['invalid-version-type', 'Cargo.toml', manifest.replace('version = "0.4.9"', 'version = ["0.4.10"]'), false],
      ['nested-metadata-content', 'Cargo.toml', manifest.replace('metadata = { version = "1.0.0"', 'metadata = { version = "2.0.0"'), false],
      ['workspace-content', 'Cargo.toml', manifest.replace('rust-version = "1.89"', 'rust-version = "1.90"'), false],
      ['member-lock-version', 'Cargo.lock', workspaceLock.replace('name = "screencomp-e2e"\nversion = "0.4.9"', 'name = "screencomp-e2e"\nversion = "0.4.10"'), true],
      ['e2e-version', 'e2e/Cargo.toml', e2e.replace('0.4.9', '0.4.10'), true],
      ['e2e-content', 'e2e/Cargo.toml', e2e.replace('publish = false', 'publish = true'), false],
      ['json-version', 'package.json', JSON.stringify({ ...pkg, version: '0.4.10' }), true],
      ['json-content', 'package.json', JSON.stringify({ ...pkg, version: '0.4.10', dependencies: { tool: '2' } }), false],
      ['json-malformed', 'package.json', '{', false],
    ];
    for (const [branch, path, content, allowed] of cases) {
      h.git('checkout', '-q', '-B', branch, base); writeFileSync(join(h.dir, path), content);
      h.git('add', '.'); h.git('commit', '-qm', branch);
      assert.equal(h.classify(event, h.git('rev-parse', 'HEAD'), { baseRef: base }).isReleaseOnly, allowed, branch);
    }
    for (const member of ['../outside', 'missing']) {
      h.git('checkout', '-q', '-B', member === 'missing' ? 'missing-member' : 'invalid-member', base);
      const badManifest = manifest.replace('"e2e"', `"${member}"`);
      writeFileSync(join(h.dir, 'Cargo.toml'), badManifest);
      h.git('add', '.'); h.git('commit', '-qm', 'bad member baseline'); const badBase = h.git('rev-parse', 'HEAD');
      writeFileSync(join(h.dir, 'Cargo.toml'), badManifest.replace('0.4.9', '0.4.10'));
      h.git('add', '.'); h.git('commit', '-qm', 'version change');
      assert.equal(h.classify(event, h.git('rev-parse', 'HEAD'), { baseRef: badBase }).macosRequired, true);
    }
  } finally { h.clean(); }
});
