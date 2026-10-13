// One compiled target binary is staged, packed, installed and run before any
// forge upload. The workflow owns the target list; this helper owns packaging.
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { chmodSync, copyFileSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { basename, isAbsolute, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { tmpdir } from 'node:os';

const run = (cmd, args, options = {}) => {
  try { return execFileSync(cmd, args, { encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'], ...options }); }
  catch (error) {
    if (error.stdout) process.stderr.write(error.stdout);
    if (error.stderr) process.stderr.write(error.stderr);
    throw new Error(`release command ${cmd} ${args.join(' ')} failed; inspect the diagnostics above and rerun that command`);
  }
};
const hash = path => createHash('sha256').update(readFileSync(path)).digest('hex');
export function executionMode({ platform, arch, target }) {
  return platform === 'darwin' && arch === 'arm64' && target.startsWith('x86_64-apple-darwin') ? 'Rosetta' : 'native';
}
function main() {
const [target, tag, outArg = 'dist/release'] = process.argv.slice(2);
if (!/^(x86_64|aarch64)-(unknown-linux-gnu|apple-darwin|pc-windows-msvc)$/.test(target ?? '') || (target === 'aarch64-pc-windows-msvc')) throw new Error(`unsupported release target '${target}'; choose a declared target from .github/workflows/release.yml`);
if (!/^v\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/.test(tag ?? '')) throw new Error(`invalid release tag '${tag}'; supply the checkout's release tag as vX.Y.Z`);
const metadata = JSON.parse(run('cargo', ['metadata', '--no-deps', '--format-version', '1', '--locked'], { encoding: 'utf8', stdio: ['ignore', 'pipe', 'inherit'] }));
if (!metadata || !Array.isArray(metadata.packages) || !metadata.packages.every(p => p && typeof p.name === 'string' && typeof p.version === 'string') || typeof metadata.target_directory !== 'string' || !isAbsolute(metadata.target_directory)) throw new Error('invalid cargo metadata packages or target_directory; run cargo metadata --no-deps --format-version 1 --locked and repair the workspace');
const pkg = metadata.packages.find(p => p.name === 'screencomp');
if (!pkg || !/^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/.test(pkg.version)) throw new Error('cargo metadata has no valid screencomp package/version; restore its workspace manifest before building');
if (`v${pkg.version}` !== tag) throw new Error(`release tag does not match Cargo version: '${tag}' versus 'v${pkg.version}'; check out the matching release tag before building`);
const out = resolve(outArg);
mkdirSync(out, { recursive: true });
run('cargo', ['build', '--release', '--locked', '--bin', 'screencomp', '--target', target], { env: { ...process.env, RUSTFLAGS: `${process.env.RUSTFLAGS ?? ''} -D warnings`.trim() } });
const exe = target.includes('windows') ? 'screencomp.exe' : 'screencomp';
const binary = join(metadata.target_directory, target, 'release', exe);
const bytes = readFileSync(binary);
const arm = target.startsWith('aarch64');
if (target.includes('linux')) {
  if (bytes.length < 64) throw new Error(`truncated ELF binary ${binary} (${bytes.length} bytes); rebuild ${target} before packaging`);
  if (bytes.toString('hex', 0, 4) !== '7f454c46' || bytes[4] !== 2 || bytes[5] !== 1 || bytes.readUInt16LE(18) !== (arm ? 183 : 62)) throw new Error(`ELF architecture mismatch at ${binary}: machine=${bytes.readUInt16LE(18)}, target=${target}; remove this target's release directory and rebuild`);
} else if (target.includes('darwin')) {
  if (bytes.length < 32) throw new Error(`truncated Mach-O binary ${binary} (${bytes.length} bytes); rebuild ${target} before packaging`);
  if (bytes.readUInt32LE(0) !== 0xfeedfacf || bytes.readUInt32LE(4) !== (arm ? 0x100000c : 0x1000007)) throw new Error(`Mach-O architecture mismatch at ${binary}: CPU=${bytes.readUInt32LE(4)}, target=${target}; remove this target's release directory and rebuild`);
} else {
  if (bytes.length < 64) throw new Error(`truncated PE binary ${binary} (${bytes.length} bytes); rebuild ${target} before packaging`);
  const pe = bytes.readUInt32LE(0x3c);
  if (pe > bytes.length - 24) throw new Error(`PE header outside ${binary}: offset=${pe}, length=${bytes.length}; rebuild ${target} before packaging`);
  if (bytes.toString('ascii', 0, 2) !== 'MZ' || bytes.readUInt32LE(pe) !== 0x4550 || bytes.readUInt16LE(pe + 4) !== 0x8664) throw new Error(`PE architecture mismatch at ${binary}: machine=${bytes.readUInt16LE(pe + 4)}, target=${target}; remove this target's release directory and rebuild`);
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
  const rosetta = executionMode({ platform: process.platform, arch: process.arch, target }) === 'Rosetta';
  const installArgs = ['scripts/install.sh', '--version', tag, '--to', installed];
  if (rosetta) run('arch', ['-x86_64', 'bash', ...installArgs], { env });
  else run('bash', installArgs, { env });
  const installedBinary = join(installed, exe);
  if (hash(binary) !== hash(installedBinary)) throw new Error(`installed binary ${installedBinary} (${hash(installedBinary)}) differs from compiled ${binary} (${hash(binary)}); inspect archive contents and installer extraction before publishing`);
  const execute = args => rosetta ? run('arch', ['-x86_64', installedBinary, ...args]) : run(installedBinary, args);
  const versionArgs = rosetta ? ['-x86_64', installedBinary, '--version'] : ['--version'];
  const actualVersion = run(rosetta ? 'arch' : installedBinary, versionArgs, { encoding: 'utf8', stdio: ['ignore', 'pipe', 'inherit'] }).trim();
  if (actualVersion !== `screencomp ${pkg.version}`) throw new Error(`installed version mismatch: '${actualVersion}', expected 'screencomp ${pkg.version}'; rebuild from the matching tag and inspect the staged archive`);
  execute(['--quiet', 'classify', '--baseline', 'tests/fixtures/baseline', '--current', 'tests/fixtures/current']);
  console.log(`packaged ${basename(archive)}`);
  writeFileSync(join(out, `${stem}.contract.json`), JSON.stringify({ schemaVersion: 1, target, tag, version: pkg.version, archive: basename(archive), checksum: `${stem}.sha256`, binarySha256: hash(binary), installedSha256: hash(installedBinary), contents: [exe, 'README.md', 'LICENSE', 'CHANGELOG.md'] }) + '\n');
} finally {
  rmSync(stage, { recursive: true, force: true });
}

}
if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main();
