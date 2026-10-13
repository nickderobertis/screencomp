import { spawnSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
const pkg = JSON.parse(readFileSync(new URL('package.json', import.meta.url), 'utf8'));
if (!pkg || typeof pkg !== 'object' || Array.isArray(pkg)) throw new Error('GitHub tooling requires a package object; fix .github/package.json');
const manager = pkg.packageManager;
if (typeof manager !== 'string' || !/^bun@\d+\.\d+\.\d+$/.test(manager)) throw new Error('GitHub tooling requires packageManager bun@X.Y.Z; fix .github/package.json');
const version = manager.slice(4);
const installed = spawnSync('bun', ['--version'], { encoding: 'utf8', shell: process.platform === 'win32' });
const native = installed.status === 0 && installed.stdout.trim() === version;
const result = spawnSync(native ? 'bun' : 'npm', native ? ['install', '--frozen-lockfile', '--cwd', '.github'] : ['exec', '--yes', `--package=bun@${version}`, '--', 'bun', 'install', '--frozen-lockfile', '--cwd', '.github'], { encoding: 'utf8', shell: process.platform === 'win32' });
if (result.status !== 0) { process.stderr.write(result.stdout ?? ''); process.stderr.write(result.stderr ?? ''); throw new Error('locked GitHub tooling install failed'); }
