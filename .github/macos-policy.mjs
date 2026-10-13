import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parse as parseToml } from 'smol-toml';
import { releaseBranchPrefix } from '../tools/workspace/ci-tier.mjs';

const version = /^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$/;
const git = (...args) => execFileSync('git', args, { encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] });

// Canonical parsed comparison removes only this workspace's package versions.
// Dependency versions, checksums, sources and dependency edges remain semantic
// content, regardless of whitespace, comments or key ordering.
function canonical(value) {
  if (value instanceof Date) return { tomlDate: value.toISOString() };
  if (Array.isArray(value)) return value.map(canonical);
  if (value && typeof value === 'object') return Object.fromEntries(Object.keys(value).sort().map(key => [key, canonical(value[key])]));
  return value;
}
function normalizedToml(text, lock) {
  const parsed = parseToml(text);
  const eraseVersion = pkg => {
    if (!pkg || !('version' in pkg)) return;
    if (!version.test(pkg.version)) throw new Error('invalid package version');
    pkg.version = '<release-version>';
  };
  if (lock) {
    if (!Array.isArray(parsed.package)) throw new Error('missing lock packages');
    for (const pkg of parsed.package) if (['screencomp', 'screencomp-e2e'].includes(pkg.name) && !('source' in pkg)) eraseVersion(pkg);
  } else {
    eraseVersion(parsed.package);
    eraseVersion(parsed.workspace?.package);
  }
  return JSON.stringify(canonical(parsed));
}
function versionOnly(path, before, after) {
  if (/(^|\/)CHANGELOG\.md$/.test(path)) return true;
  if (['Cargo.toml', 'e2e/Cargo.toml', 'Cargo.lock'].includes(path)) {
    return normalizedToml(before, path === 'Cargo.lock') === normalizedToml(after, path === 'Cargo.lock');
  }
  if (path === 'package.json') {
    const a = JSON.parse(before), b = JSON.parse(after);
    if (!version.test(a.version ?? '') || !version.test(b.version ?? '')) return false;
    delete a.version; delete b.version;
    return JSON.stringify(canonical(a)) === JSON.stringify(canonical(b));
  }
  return false;
}
export function classify({ eventName, event, base, head, prefix }) {
  const result = (isReleaseOnly, reason) => ({ isReleaseOnly, macosRequired: eventName === 'pull_request' && !isReleaseOnly, reason });
  if (eventName !== 'pull_request') return result(false, 'automatic non-PR event');
  try {
    const pr = event.pull_request;
    if (!pr || !prefix || !pr.head.ref.startsWith(prefix) || !pr.head.repo.full_name || pr.head.repo.full_name !== pr.base.repo.full_name || pr.base.repo.full_name !== event.repository.full_name) {
      return result(false, 'ordinary, fork, or unknown PR identity');
    }
    if (!base || !head || base.startsWith('-') || head.startsWith('-')) throw new Error('missing refs');
    const mergeBase = git('merge-base', base, head).trim();
    const changes = git('diff', '--name-status', '-z', '--no-renames', mergeBase, head).split('\0').filter(Boolean);
    if (!changes.length) return result(false, 'empty diff cannot authorize omission');
    for (let i = 0; i < changes.length; i += 2) {
      const [status, path] = changes.slice(i, i + 2);
      if (status !== 'M' || !versionOnly(path, git('show', `${mergeBase}:${path}`), git('show', `${head}:${path}`))) {
        return result(false, `non-release content: ${path}`);
      }
    }
    return result(true, 'same-repository release-plz branch and semantic version/changelog-only diff');
  } catch {
    return result(false, 'unreadable PR or diff; Mac coverage required');
  }
}
if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const [command, ...args] = process.argv.slice(2);
  if (command !== 'classify' || args.length !== 6 || args[0] !== '--event' || args[2] !== '--base' || args[4] !== '--head') {
    throw new Error('usage: macos-policy.mjs classify --event <JSON path> --base <git ref> --head <git ref>');
  }
  let event = null;
  try { event = JSON.parse(readFileSync(args[1], 'utf8')); } catch { /* fail closed below */ }
  const eventName = process.env.GITHUB_EVENT_NAME ?? (event?.pull_request ? 'pull_request' : 'unknown');
  let prefix = null;
  try { prefix = releaseBranchPrefix(readFileSync(new URL('workflows/release-plz.yml', import.meta.url), 'utf8')); } catch { /* missing policy authority requires Mac coverage */ }
  console.log(JSON.stringify(classify({ eventName, event, base: args[3], head: args[5], prefix })));
}
