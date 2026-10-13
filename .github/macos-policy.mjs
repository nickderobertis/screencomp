import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parse as parseToml } from 'smol-toml';
import { releaseBranchPrefix } from '../tools/workspace/ci-tier.mjs';

const knownNonPrEvents = new Set(['push', 'release', 'workflow_run', 'repository_dispatch', 'schedule', 'workflow_dispatch']);
const parseManifest = text => parseToml(text, { integersAsBigInt: true });
const version = /^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$/;
const git = (...args) => execFileSync('git', args, { encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] });

// Canonical parsed comparison removes only this workspace's package versions.
// Dependency versions, checksums, sources and dependency edges remain semantic
// content, regardless of whitespace, comments or key ordering.
function canonical(value) {
  if (value instanceof Date) return ['date', value.toISOString()];
  if (Array.isArray(value)) return ['array', value.map(canonical)];
  if (value && typeof value === 'object') return ['object', Object.keys(value).sort().map(key => [key, canonical(value[key])])];
  if (typeof value === 'number') return ['number', Object.is(value, -0) ? '-0' : String(value)];
  if (typeof value === 'bigint') return ['integer', String(value)];
  return [typeof value, value];
}
function normalizedToml(text, lock, localNames) {
  const parsed = parseManifest(text);
  const eraseVersion = pkg => {
    if (!pkg || !('version' in pkg)) return;
    if (typeof pkg.version !== 'string' || !version.test(pkg.version)) throw new Error('invalid package version');
    pkg.version = '<release-version>';
  };
  if (lock) {
    if (!Array.isArray(parsed.package)) throw new Error('missing lock packages');
    for (const pkg of parsed.package) if (localNames.has(pkg.name) && !('source' in pkg)) eraseVersion(pkg);
  } else {
    eraseVersion(parsed.package);
    eraseVersion(parsed.workspace?.package);
  }
  return JSON.stringify(canonical(parsed));
}
function versionOnly(path, before, after, localNames) {
  if (/(^|\/)CHANGELOG\.md$/.test(path)) return true;
  if (['Cargo.toml', 'e2e/Cargo.toml', 'Cargo.lock'].includes(path)) {
    return normalizedToml(before, path === 'Cargo.lock', localNames) === normalizedToml(after, path === 'Cargo.lock', localNames);
  }
  if (path === 'package.json') {
    const a = JSON.parse(before), b = JSON.parse(after);
    if (typeof a.version !== 'string' || typeof b.version !== 'string' || !version.test(a.version) || !version.test(b.version)) return false;
    delete a.version; delete b.version;
    return JSON.stringify(canonical(a)) === JSON.stringify(canonical(b));
  }
  return false;
}
function workspacePackages(base) {
  const root = parseManifest(git('show', `${base}:Cargo.toml`));
  const names = new Set();
  const add = manifest => {
    const name = manifest.package?.name;
    if (typeof name !== 'string' || !/^[A-Za-z0-9_-]+$/.test(name)) throw new Error('unknown workspace package identity');
    names.add(name);
  };
  if (root.package) add(root);
  const members = root.workspace?.members ?? [];
  if (!Array.isArray(members)) throw new Error('unknown workspace members');
  for (const member of members) {
    if (member === '.') continue;
    if (typeof member !== 'string' || !/^[A-Za-z0-9_-]+(?:\/[A-Za-z0-9_-]+)*$/.test(member)) throw new Error('unsupported workspace member');
    add(parseManifest(git('show', `${base}:${member}/Cargo.toml`)));
  }
  if (!names.size) throw new Error('no workspace package identities');
  return names;
}
export function classify({ eventName, event, base, head, prefix }) {
  const result = (isReleaseOnly, reason) => ({ isReleaseOnly, macosRequired: eventName === 'pull_request' && !isReleaseOnly, reason });
  if (knownNonPrEvents.has(eventName)) return result(false, 'known non-PR event');
  if (eventName !== 'pull_request') return { isReleaseOnly: false, macosRequired: true, reason: 'unknown event name; Mac coverage required' };
  try {
    const pr = event.pull_request;
    const repoName = /^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/;
    if (![pr?.head?.repo?.full_name, pr?.base?.repo?.full_name, event.repository?.full_name].every(name => typeof name === 'string' && repoName.test(name))) return result(false, 'unknown PR repository identity');
    if (!pr || !prefix || !pr.head.ref.startsWith(prefix) || !pr.head.repo.full_name || pr.head.repo.full_name !== pr.base.repo.full_name || pr.base.repo.full_name !== event.repository.full_name) {
      return result(false, 'ordinary, fork, or unknown PR identity');
    }
    if (!base || !head || base.startsWith('-') || head.startsWith('-')) throw new Error('missing refs');
    const mergeBase = git('merge-base', base, head).trim();
    const localNames = workspacePackages(mergeBase);
    const changes = git('diff', '--name-status', '-z', '--no-renames', mergeBase, head).split('\0').filter(Boolean);
    if (!changes.length) return result(false, 'empty diff cannot authorize omission');
    for (let i = 0; i < changes.length; i += 2) {
      const [status, path] = changes.slice(i, i + 2);
      if (status !== 'M' || !versionOnly(path, git('show', `${mergeBase}:${path}`), git('show', `${head}:${path}`), localNames)) {
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
  const inferred = event && typeof event === 'object' && !Array.isArray(event) && 'pull_request' in event ? 'pull_request'
    : typeof event?.release?.tag_name === 'string' ? 'release'
    : typeof event?.ref === 'string' && /^refs\/(heads|tags)\//.test(event.ref) && typeof event.before === 'string' && /^[0-9a-f]{40}([0-9a-f]{24})?$/.test(event.before) ? 'push'
    : 'pull_request';
  const eventName = process.env.GITHUB_EVENT_NAME ?? inferred;
  let prefix = null;
  try { prefix = releaseBranchPrefix(readFileSync(new URL('workflows/release-plz.yml', import.meta.url), 'utf8')); } catch { /* missing policy authority requires Mac coverage */ }
  console.log(JSON.stringify(classify({ eventName, event, base: args[3], head: args[5], prefix })));
}
