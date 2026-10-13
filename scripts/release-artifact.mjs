// One compiled target binary is staged, packed, installed and run before any
// forge upload. The workflow owns the target list; this helper owns packaging.
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { chmodSync, copyFileSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { basename, join, resolve } from 'node:path';
import { tmpdir } from 'node:os';

const run = (cmd, args, options = {}) => execFileSync(cmd, args, { stdio: 'inherit', ...options });
const hash = path => createHash('sha256').update(readFileSync(path)).digest('hex');
const [target, tag, outArg = 'dist/release'] = process.argv.slice(2);
if (!/^(x86_64|aarch64)-(unknown-linux-gnu|apple-darwin|pc-windows-msvc)$/.test(target ?? '') || (target === 'aarch64-pc-windows-msvc')) throw new Error('unsupported release target');
if (!/^v\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/.test(tag ?? '')) throw new Error('expected release tag vX.Y.Z');
const metadata = JSON.parse(run('cargo', ['metadata', '--no-deps', '--format-version', '1', '--locked'], { encoding: 'utf8', stdio: ['ignore', 'pipe', 'inherit'] }));
const pkg = metadata.packages.find(p => p.name === 'screencomp');
if (`v${pkg.version}` !== tag) throw new Error('release tag does not match Cargo version');
const out = resolve(outArg);
mkdirSync(out, { recursive: true });
run('cargo', ['build', '--release', '--locked', '--bin', 'screencomp', '--target', target]);
const exe = target.includes('windows') ? 'screencomp.exe' : 'screencomp';
const binary = join(metadata.target_directory, target, 'release', exe);
const bytes = readFileSync(binary);
const arm = target.startsWith('aarch64');
if (target.includes('linux')) {
  if (bytes.toString('hex', 0, 4) !== '7f454c46' || bytes[4] !== 2 || bytes[5] !== 1 || bytes.readUInt16LE(18) !== (arm ? 183 : 62)) throw new Error('ELF architecture mismatch');
} else if (target.includes('darwin')) {
  if (bytes.readUInt32LE(0) !== 0xfeedfacf || bytes.readUInt32LE(4) !== (arm ? 0x100000c : 0x1000007)) throw new Error('Mach-O architecture mismatch');
} else {
  const pe = bytes.readUInt32LE(0x3c);
  if (bytes.toString('ascii', 0, 2) !== 'MZ' || bytes.readUInt32LE(pe) !== 0x4550 || bytes.readUInt16LE(pe + 4) !== 0x8664) throw new Error('PE architecture mismatch');
}
const stage = mkdtempSync(join(tmpdir(), 'screencomp-release-'));
try {
  copyFileSync(binary, join(stage, exe));
  for (const doc of ['README.md', 'LICENSE', 'CHANGELOG.md']) copyFileSync(doc, join(stage, doc));
  const stem = `screencomp-${tag}-${target}`;
  const archive = join(out, `${stem}.${target.includes('windows') ? 'zip' : 'tar.gz'}`);
  if (target.includes('windows')) run('powershell.exe', ['-NoProfile', '-Command', 'Compress-Archive -Path "$env:SCREENCOMP_STAGE/*" -DestinationPath $env:SCREENCOMP_ARCHIVE -Force'], { env: { ...process.env, SCREENCOMP_STAGE: stage, SCREENCOMP_ARCHIVE: archive } });
  else run('tar', ['-czf', archive, '-C', stage, exe, 'README.md', 'LICENSE', 'CHANGELOG.md']);
  writeFileSync(join(out, `${stem}.sha256`), `${hash(archive)}  ${basename(archive)}\n`);
  // Curl is the forge transport boundary. The real installer still detects its
  // platform, verifies the checksum, extracts and installs the staged artifact.
  const transport = join(stage, 'transport');
  mkdirSync(transport);
  writeFileSync(join(transport, 'curl'), `#!/bin/sh\nset -eu\n[ "$1" = -fsSL ] && [ "$2" = -o ]\nasset="\${4##*/}"\ncp "$SCREENCOMP_STAGED_ASSETS/$asset" "$3"\n`);
  chmodSync(join(transport, 'curl'), 0o755);
  const installed = join(stage, 'installed');
  const env = { ...process.env, SCREENCOMP_STAGED_ASSETS: out, PATH: `${transport}${process.platform === 'win32' ? ';' : ':'}${process.env.PATH}` };
  const rosetta = process.platform === 'darwin' && target.startsWith('x86_64') && process.arch === 'arm64';
  const installArgs = ['scripts/install.sh', '--version', tag, '--to', installed];
  if (rosetta) run('arch', ['-x86_64', 'bash', ...installArgs], { env });
  else run('bash', installArgs, { env });
  const installedBinary = join(installed, exe);
  if (hash(binary) !== hash(installedBinary)) throw new Error('installed binary differs from compiled target');
  const execute = args => rosetta ? run('arch', ['-x86_64', installedBinary, ...args]) : run(installedBinary, args);
  const versionArgs = rosetta ? ['-x86_64', installedBinary, '--version'] : ['--version'];
  const actualVersion = run(rosetta ? 'arch' : installedBinary, versionArgs, { encoding: 'utf8', stdio: ['ignore', 'pipe', 'inherit'] }).trim();
  if (actualVersion !== `screencomp ${pkg.version}`) throw new Error(`installed version mismatch: ${actualVersion}`);
  execute(['classify', '--baseline', 'tests/fixtures/baseline', '--current', 'tests/fixtures/current']);
  writeFileSync(join(out, `${stem}.contract.json`), JSON.stringify({ schemaVersion: 1, target, tag, version: pkg.version, archive: basename(archive), checksum: `${stem}.sha256`, binarySha256: hash(binary), installedSha256: hash(installedBinary), contents: [exe, 'README.md', 'LICENSE', 'CHANGELOG.md'] }) + '\n');
} finally {
  rmSync(stage, { recursive: true, force: true });
}
