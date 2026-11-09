import assert from 'node:assert/strict';
import { readFileSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { test } from 'node:test';

const workflows = resolve(import.meta.dirname, '../workflows');
const fetchers = /(^|[\s|;&(])(curl|wget)(\s|$)/;

test('no workflow downloads outside scripts/download.sh', () => {
  const bare = readdirSync(workflows).flatMap(name =>
    readFileSync(join(workflows, name), 'utf8').split('\n')
      .filter(line => !line.trim().startsWith('#') && fetchers.test(line))
      .map(line => `${name}: ${line.trim()}`));
  assert.deepEqual(bare, []);
});
