import { execFileSync } from 'node:child_process';
import { mkdtempSync, mkdirSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { root } from '../workflow-graph.mjs';
export function history() {
  const dir = mkdtempSync(join(tmpdir(), 'screencomp-policy-'));
  const git = (...args) => execFileSync('git', args, { cwd: dir, encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] }).trim();
  git('init', '-q'); git('config', 'user.email', 'test@example.invalid'); git('config', 'user.name', 'Test');
  const manifest = '[package]\nname = "screencomp"\nversion = "0.4.9"\n[dependencies]\nserde = "1"\n';
  const lock = 'version = 4\n[[package]]\nname = "screencomp"\nversion = "0.4.9"\ndependencies = ["serde"]\n[[package]]\nname = "serde"\nversion = "1.0.0"\nsource = "registry"\nchecksum = "abc"\n';
  writeFileSync(join(dir, 'Cargo.toml'), manifest); writeFileSync(join(dir, 'Cargo.lock'), lock);
  writeFileSync(join(dir, 'CHANGELOG.md'), '# Changelog\n'); mkdirSync(join(dir, 'src')); writeFileSync(join(dir, 'src/lib.rs'), 'pub fn old() {}\n');
  git('add', '.'); git('commit', '-qm', 'initial'); const base = git('rev-parse', 'HEAD');
  const heads = {};
  for (const kind of ['source', 'version-only', 'dependency-content']) {
    git('checkout', '-q', '-B', kind, base);
    if (kind === 'source') writeFileSync(join(dir, 'src/lib.rs'), 'pub fn changed() {}\n');
    if (kind === 'version-only') {
      writeFileSync(join(dir, 'Cargo.toml'), manifest.replace('0.4.9', '0.4.10'));
      writeFileSync(join(dir, 'Cargo.lock'), lock.replace('0.4.9', '0.4.10'));
      writeFileSync(join(dir, 'CHANGELOG.md'), '# Changelog\n\n## 0.4.10\nChanges.\n');
    }
    if (kind === 'dependency-content') writeFileSync(join(dir, 'Cargo.toml'), manifest.replace('serde = "1"', 'serde = "2"'));
    git('add', '.'); git('commit', '-qm', kind); heads[kind] = git('rev-parse', 'HEAD');
  }
  const classify = (event, head, { eventName = 'pull_request', baseRef = base, raw } = {}) => {
    const path = join(dir, '.git', 'event.json'); writeFileSync(path, raw ?? JSON.stringify(event));
    return JSON.parse(execFileSync(process.execPath, [join(root, '.github/macos-policy.mjs'), 'classify', '--event', path, '--base', baseRef, '--head', head], { cwd: dir, encoding: 'utf8', env: { ...process.env, GITHUB_EVENT_NAME: eventName } }));
  };
  return { dir, git, base, heads, classify, clean: () => rmSync(dir, { recursive: true, force: true }) };
}
export function prEvent({ branch = 'fix/ordinary', fork = false, author = 'nickderobertis', action = 'synchronize', base, head } = {}) {
  return { action, repository: { full_name: 'nickderobertis/screencomp' }, pull_request: { title: 'chore: release v0.4.10', user: { login: author }, head: { ref: branch, sha: head, repo: { full_name: fork ? 'someone/screencomp' : 'nickderobertis/screencomp' } }, base: { ref: 'main', sha: base, repo: { full_name: 'nickderobertis/screencomp' } } } };
}
