import assert from 'node:assert/strict';
import { readFileSync, writeFileSync } from 'node:fs';
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
    for (const eventName of ['push', 'release', 'schedule', 'workflow_run', 'repository_dispatch', 'workflow_dispatch']) assert.equal(h.classify({}, h.heads.source, { eventName, raw: '{' }).macosRequired, false);
    for (const [name, content] of [
      ['malformed', '[package]\nname = "screencomp"\nversion = "0.4.10"\n[broken'],
      ['nested-version', '[package]\nname = "screencomp"\nversion = "0.4.10"\nmetadata = { version = "1.0.0" }\n[dependencies]\nserde = "1"\n'],
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
    // A source edit on an otherwise valid release branch cannot hide behind a version bump.
    h.git('checkout', '-q', '-B', 'mixed', h.heads['version-only']);
    writeFileSync(join(h.dir, 'src/lib.rs'), 'pub fn mixed() {}');
    h.git('add', '.'); h.git('commit', '-qm', 'mixed');
    assert.equal(h.classify(prEvent({ branch: 'release-plz-x' }), h.git('rev-parse', 'HEAD')).macosRequired, true);
  } finally { h.clean(); }
});
