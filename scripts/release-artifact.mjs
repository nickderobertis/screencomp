// The workflow owns the target list. Each compiled binary is packed, installed
// and run on its native runner before Linux publication. The build mode does
// all three; `pack` and `verify-install` are its last two halves as separate
// commands, so a job can pack without executing and verify the packed archive
// as its own step. Both default to the binary cargo built for the target.
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
    const reason = error.code ?? error.signal ?? error.status ?? 'unknown';
    const detail = error.stdout || error.stderr ? `exit ${reason}; inspect the diagnostics above` : `${error.message} (${reason})`;
    throw new Error(`release command ${cmd} ${args.join(' ')} failed: ${detail}; rerun that command after correcting the cause`);
  }
};
const hash = path => createHash('sha256').update(readFileSync(path)).digest('hex');
const binName = target => target.includes('windows') ? 'screencomp.exe' : 'screencomp';
function validateTarget(target) {
  if (target === 'x86_64-apple-darwin') throw new Error(`Intel macOS (x86_64-apple-darwin) is no longer released; build aarch64-apple-darwin for Apple Silicon`);
  if (!/^(x86_64|aarch64)-(unknown-linux-gnu|apple-darwin|pc-windows-msvc)$/.test(target ?? '') || target === 'aarch64-pc-windows-msvc') throw new Error(`unsupported release target '${target}'; choose a declared target from .github/workflows/release.yml`);
}
function verifyBinary(binary, target) {
  let bytes;
  try { bytes = readFileSync(binary); } catch { throw new Error(`cannot read compiled binary ${binary}; rebuild ${target} before packaging`); }
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
}
function metadataFor(tag) {
  if (!/^v\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/.test(tag ?? '')) throw new Error(`invalid release tag '${tag}'; supply the checkout's release tag as vX.Y.Z`);
  const metadata = JSON.parse(run('cargo', ['metadata', '--no-deps', '--format-version', '1', '--locked']));
  if (!metadata || !Array.isArray(metadata.packages) || !metadata.packages.every(p => p && typeof p.name === 'string' && typeof p.version === 'string') || typeof metadata.target_directory !== 'string' || !isAbsolute(metadata.target_directory)) throw new Error('invalid cargo metadata packages or target_directory; run cargo metadata --no-deps --format-version 1 --locked and repair the workspace');
  const pkg = metadata.packages.find(p => p.name === 'screencomp');
  if (!pkg || !/^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/.test(pkg.version)) throw new Error('cargo metadata has no valid screencomp package/version; restore its workspace manifest before building');
  if (`v${pkg.version}` !== tag) throw new Error(`release tag does not match Cargo version: '${tag}' versus 'v${pkg.version}'; check out the matching release tag before building`);
  return { targetDirectory: metadata.target_directory, version: pkg.version };
}
function verifyInstall({ binary, target, tag, out, version }) {
  verifyBinary(binary, target);
  const stage = mkdtempSync(join(tmpdir(), 'screencomp-install-'));
  try {
    // Only forge transport is substituted: install.sh still detects the host,
    // verifies the archive checksum, extracts and installs the real product.
    const transport = join(stage, 'transport');
    mkdirSync(transport);
    writeFileSync(join(transport, 'curl'), `#!/bin/sh\nset -eu\n[ "$1" = -fsSL ] && [ "$2" = -o ]\nasset="\${4##*/}"\ncp "$SCREENCOMP_STAGED_ASSETS/$asset" "$3"\n`);
    chmodSync(join(transport, 'curl'), 0o755);
    const installed = join(stage, 'installed');
    const env = { ...process.env, SCREENCOMP_STAGED_ASSETS: out, PATH: `${transport}${process.platform === 'win32' ? ';' : ':'}${process.env.PATH}` };
    run('bash', ['scripts/install.sh', '--version', tag, '--to', installed], { env });
    const installedBinary = join(installed, binName(target));
    const binarySha256 = hash(binary), installedSha256 = hash(installedBinary);
    if (binarySha256 !== installedSha256) throw new Error(`installed binary ${installedBinary} (${installedSha256}) differs from compiled ${binary} (${binarySha256}); inspect archive contents and installer extraction before publishing`);
    const actualVersion = run(installedBinary, ['--version']).trim();
    if (actualVersion !== `screencomp ${version}`) throw new Error(`installed version mismatch: '${actualVersion}', expected 'screencomp ${version}'; rebuild from the matching tag and inspect the staged archive`);
    run(installedBinary, ['--quiet', 'classify', '--baseline', 'tests/fixtures/baseline', '--current', 'tests/fixtures/current']);
    return { binarySha256, installedSha256 };
  } finally { rmSync(stage, { recursive: true, force: true }); }
}
// The one packing implementation for every release: the flat archive, its
// sha256sum-format checksum, and the per-artifact contract. `installedSha256`
// is present only when the archive was installed and run on its native host.
function packArtifact({ binary, target, tag, version, out, verify }) {
  const exe = binName(target), stem = `screencomp-${tag}-${target}`;
  const stage = mkdtempSync(join(tmpdir(), 'screencomp-release-'));
  try {
    mkdirSync(out, { recursive: true });
    // llmlint: ignore-block[work_goes_through_command_surface] Released assets follow the published v-tag, flat-archive, stem.sha256 contract, which differs from the local dist-build layout; unifying the two interfaces is out of scope here.
    copyFileSync(binary, join(stage, exe));
    for (const doc of ['README.md', 'LICENSE', 'CHANGELOG.md']) copyFileSync(doc, join(stage, doc));
    const archive = join(out, `${stem}.${target.includes('windows') ? 'zip' : 'tar.gz'}`);
    if (target.includes('windows')) run('powershell.exe', ['-NoProfile', '-Command', 'Compress-Archive -Path "$env:SCREENCOMP_STAGE/*" -DestinationPath $env:SCREENCOMP_ARCHIVE -Force'], { env: { ...process.env, SCREENCOMP_STAGE: stage, SCREENCOMP_ARCHIVE: archive } });
    // GNU tar reads a `C:` archive path as a remote host, so it is named
    // relative to the output directory.
    else run('tar', ['-czf', basename(archive), '-C', stage, exe, 'README.md', 'LICENSE', 'CHANGELOG.md'], { cwd: out });
    writeFileSync(join(out, `${stem}.sha256`), `${hash(archive)}  ${basename(archive)}\n`);
    // llmlint: ignore-end[work_goes_through_command_surface]
    const hashes = verify ? verifyInstall({ binary, target, tag, out, version }) : { binarySha256: hash(binary) };
    writeFileSync(join(out, `${stem}.contract.json`), JSON.stringify({ schemaVersion: 2, target, tag, version, archive: basename(archive), checksum: `${stem}.sha256`, ...hashes, contents: [exe, 'README.md', 'LICENSE', 'CHANGELOG.md'] }) + '\n');
    console.log(`packaged ${basename(archive)}`);
  } finally { rmSync(stage, { recursive: true, force: true }); }
}
function main() {
  let args = process.argv.slice(2);
  const mode = ['verify-binary', 'verify-install', 'pack'].includes(args[0]) ? args.shift() : 'build';
  if (mode === 'verify-binary') {
    const [target, binary] = args;
    validateTarget(target);
    if (args.length !== 2) throw new Error('usage: release-artifact.mjs verify-binary <target> <compiled binary>');
    verifyBinary(binary, target);
    console.log(`verified binary ${binary}`);
    return;
  }
  const [target, tag, outArg = 'dist/release', suppliedBinary] = args;
  validateTarget(target);
  const { targetDirectory, version } = metadataFor(tag);
  const out = resolve(outArg);
  const binary = suppliedBinary === undefined ? join(targetDirectory, target, 'release', binName(target)) : resolve(suppliedBinary);
  if (mode === 'verify-install') {
    if (args.length < 2 || args.length > 4) throw new Error('usage: release-artifact.mjs verify-install <target> <tag> [staged assets] [compiled binary]');
    const contractPath = join(out, `screencomp-${tag}-${target}.contract.json`);
    let contract;
    try { contract = JSON.parse(readFileSync(contractPath, 'utf8')); } catch { throw new Error(`cannot read packed contract ${contractPath}; run release-artifact.mjs pack ${target} ${tag} first`); }
    const hashes = verifyInstall({ binary, target, tag, out, version });
    writeFileSync(contractPath, JSON.stringify({ ...contract, ...hashes }) + '\n');
    console.log(`verified installer ${target} ${tag}`);
    return;
  }
  if (mode === 'pack') {
    if (args.length < 2 || args.length > 4) throw new Error('usage: release-artifact.mjs pack <target> <tag> [output directory] [compiled binary]');
    verifyBinary(binary, target);
    packArtifact({ binary, target, tag, version, out, verify: false });
    return;
  }
  if (args.length < 2 || args.length > 3) throw new Error('usage: release-artifact.mjs <target> <tag> [output directory]');
  mkdirSync(out, { recursive: true });
  run('just', ['build-release', target]);
  verifyBinary(binary, target);
  packArtifact({ binary, target, tag, version, out, verify: true });
}
if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try { main(); } catch (error) {
    console.error(`release-artifact: ${error.message}; inspect the named path or metadata, correct permissions/content and rerun the same release-artifact command before publishing`);
    process.exitCode = 1;
  }
}
