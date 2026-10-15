import assert from 'node:assert/strict';
import { execFileSync, spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { cpSync, existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { parse } from 'yaml';
import { chmodSync } from 'node:fs';
import { test } from 'node:test';
const root = resolve(import.meta.dirname, '../..');
const helper = join(root, 'scripts/release-artifact.mjs');
const pkgName = metadata => metadata.packages.find(p => p.name === 'screencomp').name;
const sha256 = path => createHash('sha256').update(readFileSync(path)).digest('hex');
const metadataVersion = () => JSON.parse(execFileSync('cargo', ['metadata', '--no-deps', '--format-version', '1', '--locked'], { cwd: root, encoding: 'utf8' })).packages.find(p => p.name === 'screencomp').version;
// A Mach-O header for the given CPU: enough for the architecture check, never
// executed. Linux and Windows hosts cannot compile or run a Darwin binary.
function machoFixture(path, cpu) {
  const macho = Buffer.alloc(32); macho.writeUInt32LE(0xfeedfacf, 0); macho.writeUInt32LE(cpu, 4);
  writeFileSync(path, macho);
  return path;
}
const ARM64 = 0x100000c, X86_64 = 0x1000007;
// The test-only `pack` mode: packs a binary compiled elsewhere through the same
// packArtifact that release.yml's build mode uses, without executing it.
const pack = (target, tag, out, binary) => spawnSync(process.execPath, [helper, 'pack', target, tag, out, binary], { cwd: root, encoding: 'utf8' });
// Every release.yml producer job with the targets it packs and its upload step.
function releaseProducers() {
  const { jobs } = parse(readFileSync(join(root, '.github/workflows/release.yml'), 'utf8'));
  return { jobs, producers: [
    ...jobs.build.strategy.matrix.include.map(row => ({ targets: [row.target], step: jobs.build.steps.find(s => s.uses === 'actions/upload-artifact@v4'), matrix: row })),
    { targets: jobs['build-darwin'].env.DARWIN_TARGETS.split(/\s+/), step: jobs['build-darwin'].steps.find(s => s.uses === 'actions/upload-artifact@v4'), matrix: {} },
  ] };
}
// Runs release.yml's artifact download selection and its publication command
// over the staged per-job artifacts; only the forge transport (`gh`, and the
// artifact service as a directory) is doubled.
function publishStaged({ jobs, artifactStore, work, tag, env, fail = false }) {
  const download = jobs.upload.steps.find(s => s.uses === 'actions/download-artifact@v4');
  assert.equal(download.with['merge-multiple'], true);
  assert.match(download.with.pattern, /^[A-Za-z0-9_-]+\*$/);
  const publishingRoot = join(work, 'linux-publication'); const downloaded = join(publishingRoot, download.with.path);
  mkdirSync(downloaded, { recursive: true });
  const prefix = download.with.pattern.slice(0, -1);
  for (const artifact of readdirSync(artifactStore).filter(name => name.startsWith(prefix))) for (const file of readdirSync(join(artifactStore, artifact))) cpSync(join(artifactStore, artifact, file), join(downloaded, file));
  const transport = join(work, 'forge-publish-transport'); mkdirSync(transport, { recursive: true });
  const request = join(work, 'forge-upload.json');
  writeFileSync(join(transport, 'gh'), '#!/usr/bin/env node\nconst fs = require("node:fs"); const args = process.argv.slice(2); if (process.env.FORGE_FAIL === "true") { console.error("forge upload unavailable"); process.exit(1); } for (const file of args.slice(3)) if (!fs.existsSync(file)) process.exit(1); fs.writeFileSync(process.env.FORGE_REQUEST, JSON.stringify({args, repo:process.env.GH_REPO}));\n');
  chmodSync(join(transport, 'gh'), 0o755);
  const publish = jobs.upload.steps.find(s => s.name === 'Publish verified archives and checksums');
  const result = spawnSync('bash', ['-c', publish.run], { cwd: publishingRoot, encoding: 'utf8', env: { ...env, PATH: `${transport}${process.platform === 'win32' ? ';' : ':'}${env.PATH}`, TAG: tag, GH_TOKEN: 'forge-double', GH_REPO: 'nickderobertis/screencomp', FORGE_REQUEST: request, FORGE_FAIL: String(fail) } });
  const uploadRequest = result.status === 0 ? JSON.parse(readFileSync(request, 'utf8')) : null;
  return { result, downloaded, uploadRequest, published: uploadRequest?.args.slice(3).map(path => path.replaceAll('\\', '/').split('/').at(-1)) };
}
test('release production commands build, pack, checksum, install and run the real host artifact', { timeout: 240000 }, () => {
  const out = mkdtempSync(join(tmpdir(), 'screencomp-packaging-test-'));
  try {
    const target = /^host: (.*)$/m.exec(execFileSync('rustc', ['-vV'], { encoding: 'utf8' }))[1];
    const metadata = JSON.parse(execFileSync('cargo', ['metadata', '--no-deps', '--format-version', '1', '--locked'], { cwd: root, encoding: 'utf8' }));
    const version = metadata.packages.find(p => p.name === 'screencomp').version;
    const tag = `v${version}`;
    const invalid = spawnSync(process.execPath, [helper, target, 'v999.0.0', out], { cwd: root, encoding: 'utf8' });
    assert.notEqual(invalid.status, 0); assert.match(invalid.stderr, /tag does not match Cargo version/);
    assert.deepEqual(readdirSync(out), []);
    const built = spawnSync(process.execPath, [helper, target, tag, out], { cwd: root, encoding: 'utf8', timeout: 230000 });
    assert.equal(built.status, 0, built.stdout + built.stderr);
    const stem = `screencomp-${tag}-${target}`;
    const contract = JSON.parse(readFileSync(join(out, `${stem}.contract.json`), 'utf8'));
    const golden = JSON.parse(readFileSync(join(root, '.github/release-contract.json'), 'utf8'));
    assert.ok(golden.targets.includes(target));
    assert.equal(pkgName(metadata), golden.binary);
    const expectedStem = golden.archiveStem.replace('$tag', tag).replace('$target', target);
    const extension = target.includes('windows') ? golden.windowsExtension : golden.unixExtension;
    assert.equal(contract.archive, `${expectedStem}.${extension}`);
    assert.equal(contract.checksum, `${expectedStem}.${golden.checksumExtension}`);
    const forms = readdirSync(out).some(file => file === contract.archive) ? ['archive'] : [];
    assert.deepEqual(forms, golden.packagingForms);
    assert.deepEqual(readdirSync(out).sort(), [contract.archive, contract.checksum, `${expectedStem}.contract.json`].sort());
    assert.equal(contract.schemaVersion, golden.schemaVersion);
    assert.equal(contract.tag, tag); assert.equal(contract.version, version); assert.equal(contract.target, target);
    const expectedContents = target.includes('windows') ? golden.contents.map(f => f === 'screencomp' ? golden.windowsBinary : f) : golden.contents;
    assert.deepEqual(contract.contents, expectedContents);
    assert.equal(contract.binarySha256, contract.installedSha256);
    const archive = join(out, contract.archive);
    const sum = createHash('sha256').update(readFileSync(archive)).digest('hex');
    assert.equal(readFileSync(join(out, contract.checksum), 'utf8'), `${sum}  ${contract.archive}\n`);
    const listing = target.includes('windows')
      ? execFileSync('powershell.exe', ['-NoProfile', '-Command', 'Add-Type -AssemblyName System.IO.Compression.FileSystem; $zip = [System.IO.Compression.ZipFile]::OpenRead($env:SCREENCOMP_ARCHIVE); try { $zip.Entries | ForEach-Object { $_.FullName } } finally { $zip.Dispose() }'], { encoding: 'utf8', env: { ...process.env, SCREENCOMP_ARCHIVE: archive } })
      : execFileSync('tar', ['-tf', archive], { encoding: 'utf8' });
    const contents = listing.trim().split(/\r?\n/);
    assert.deepEqual(contents.sort(), [...expectedContents].sort());
    assert.equal(built.stdout, `packaged ${contract.archive}\n`);
    assert.equal(built.stderr, '');
    const nativeBinary = join(metadata.target_directory, target, 'release', target.includes('windows') ? golden.windowsBinary : golden.binary);
    const action = parse(readFileSync(join(root, 'action.yml'), 'utf8'));
    const installStep = action.runs.steps.find(s => s.name === 'Install screencomp');
    const actionTemp = join(out, 'action-runner'); const transport = join(out, 'forge-transport');
    mkdirSync(actionTemp); mkdirSync(transport);
    writeFileSync(join(transport, 'curl'), '#!/bin/sh\nset -eu\nasset="${4##*/}"\ncp "$STAGED_ASSETS/$asset" "$3"\n');
    chmodSync(join(transport, 'curl'), 0o755);
    const pathFile = join(actionTemp, 'path');
    const actionEnv = { ...process.env, PATH: `${transport}${process.platform === 'win32' ? ';' : ':'}${process.env.PATH}`, STAGED_ASSETS: out, SCREENCOMP_METHOD: 'download', SCREENCOMP_VERSION: tag, SCREENCOMP_REPO: 'nickderobertis/screencomp', SCREENCOMP_ACTION_PATH: root, RUNNER_TEMP: actionTemp, TMPDIR: actionTemp, GITHUB_PATH: pathFile, GH_TOKEN: 'forge-double' };
    const installedAction = spawnSync('bash', ['-c', installStep.run], { cwd: root, env: actionEnv, encoding: 'utf8' });
    assert.equal(installedAction.status, 0, installedAction.stdout + installedAction.stderr);
    assert.match(installedAction.stdout, new RegExp(`screencomp ${version.replaceAll('.', '\\.')}`));
    const actionBinary = join(readFileSync(pathFile, 'utf8').trim(), target.includes('windows') ? golden.windowsBinary : golden.binary);
    assert.equal(createHash('sha256').update(readFileSync(actionBinary)).digest('hex'), contract.binarySha256);
    const { jobs, producers } = releaseProducers();
    const artifactStore = join(out, 'forge-artifact-store'); mkdirSync(artifactStore);
    const wanted = [];
    for (const producer of producers) {
      const name = producer.step.with.name.replace('${{ matrix.target }}', producer.matrix.target ?? '');
      assert.ok(!name.includes('${{'), 'unresolved artifact name');
      assert.equal(producer.step.with.path, 'dist/release/*');
      const artifactDir = join(artifactStore, name); mkdirSync(artifactDir);
      for (const publishedTarget of producer.targets) {
        const stem = golden.archiveStem.replace('$tag', tag).replace('$target', publishedTarget);
        const archiveName = `${stem}.${publishedTarget.includes('windows') ? golden.windowsExtension : golden.unixExtension}`;
        const sumName = `${stem}.${golden.checksumExtension}`;
        // Other lanes' artifacts are proved by their own native runs; here
        // they are opaque forge payloads.
        for (const file of [archiveName, sumName, `${stem}.contract.json`]) {
          if (publishedTarget === target) cpSync(join(out, file), join(artifactDir, file));
          else writeFileSync(join(artifactDir, file), `forge transport payload for ${publishedTarget}`);
        }
        wanted.push(archiveName, sumName);
      }
    }
    const { result: uploaded, uploadRequest, published } = publishStaged({ jobs, artifactStore, work: out, tag, env: actionEnv });
    assert.equal(uploaded.status, 0, uploaded.stderr);
    assert.deepEqual(uploadRequest.args.slice(0, 3), ['release', 'upload', tag]);
    assert.equal(uploadRequest.repo, 'nickderobertis/screencomp');
    assert.deepEqual(published.sort(), wanted.sort());
    assert.ok(published.every(file => !file.endsWith('.contract.json')));
    const failed = publishStaged({ jobs, artifactStore, work: join(out, 'failed-publication'), tag, env: actionEnv, fail: true });
    assert.notEqual(failed.result.status, 0); assert.match(failed.result.stderr, /forge upload unavailable/);



    const verifyInstall = expectedBinary => spawnSync(process.execPath, [helper, 'verify-install', target, tag, out, expectedBinary], { cwd: root, encoding: 'utf8' });
    const checksumPath = join(out, contract.checksum);
    const originalArchive = readFileSync(archive), originalChecksum = readFileSync(checksumPath);
    writeFileSync(checksumPath, '0'.repeat(64));
    const badChecksum = verifyInstall(nativeBinary);
    assert.notEqual(badChecksum.status, 0); assert.match(badChecksum.stderr, /checksum mismatch/);
    writeFileSync(checksumPath, originalChecksum);
    const stage = join(out, 'unpacked'); mkdirSync(stage);
    if (target.includes('windows')) execFileSync('powershell.exe', ['-NoProfile', '-Command', 'Expand-Archive -Path $env:SCREENCOMP_ARCHIVE -DestinationPath $env:SCREENCOMP_STAGE -Force'], { env: { ...process.env, SCREENCOMP_ARCHIVE: archive, SCREENCOMP_STAGE: stage } });
    else execFileSync('tar', ['-xzf', archive, '-C', stage]);
    const payload = join(stage, target.includes('windows') ? golden.windowsBinary : golden.binary);
    const repack = () => {
      if (target.includes('windows')) execFileSync('powershell.exe', ['-NoProfile', '-Command', 'Compress-Archive -Path "$env:SCREENCOMP_STAGE/*" -DestinationPath $env:SCREENCOMP_ARCHIVE -Force'], { env: { ...process.env, SCREENCOMP_ARCHIVE: archive, SCREENCOMP_STAGE: stage } });
      else execFileSync('tar', ['-czf', archive, '-C', stage, ...expectedContents]);
      writeFileSync(checksumPath, `${createHash('sha256').update(readFileSync(archive)).digest('hex')}  ${contract.archive}\n`);
    };
    const changedBytes = Buffer.from(readFileSync(payload)); changedBytes[changedBytes.length - 1] ^= 1;
    writeFileSync(payload, changedBytes); repack();
    const wrongBytes = verifyInstall(nativeBinary);
    assert.notEqual(wrongBytes.status, 0); assert.match(wrongBytes.stderr, /differs from compiled/);

    // A real alternate-version build reaches version rejection after checksum
    // and byte identity succeed; compiler and installer remain the real tools.
    const clone = join(out, 'alternate-version'); mkdirSync(clone);
    for (const path of ['Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml', '.cargo', 'src', 'e2e', 'benches', 'README.md', 'LICENSE', 'CHANGELOG.md', 'justfile']) cpSync(join(root, path), join(clone, path), { recursive: true });
    const manifestPath = join(clone, 'Cargo.toml'), lockPath = join(clone, 'Cargo.lock');
    const alternateVersion = '999.0.0';
    writeFileSync(manifestPath, readFileSync(manifestPath, 'utf8').replace(/^version = "[^"]+"/m, `version = "${alternateVersion}"`));
    writeFileSync(lockPath, readFileSync(lockPath, 'utf8').replace(/(name = "screencomp"\nversion = ")[^"]+("\n)/, `$1${alternateVersion}$2`));
    execFileSync('just', ['build-release', target], { cwd: clone, env: { ...process.env, CARGO_NET_OFFLINE: 'true' }, stdio: ['ignore', 'pipe', 'pipe'], timeout: 150000 });
    const alternateMetadata = JSON.parse(execFileSync('cargo', ['metadata', '--no-deps', '--format-version', '1', '--locked'], { cwd: clone, encoding: 'utf8' }));
    const alternateBinary = join(alternateMetadata.target_directory, target, 'release', target.includes('windows') ? golden.windowsBinary : golden.binary);
    cpSync(alternateBinary, payload); repack();
    const wrongVersion = verifyInstall(alternateBinary);
    assert.notEqual(wrongVersion.status, 0); assert.match(wrongVersion.stderr, /installed version mismatch/);
    assert.match(wrongVersion.stderr, /screencomp 999.0.0/);
    writeFileSync(archive, originalArchive); writeFileSync(checksumPath, originalChecksum);
    const recovered = verifyInstall(nativeBinary);
    assert.equal(recovered.status, 0, recovered.stderr);
    assert.equal(recovered.stdout, `verified installer ${target} ${tag}\n`);

  } finally { rmSync(out, { recursive: true, force: true, maxRetries: 5, retryDelay: 100 }); }
});

test('the Apple Silicon artifact is packed by the shared release packer and is the only macOS asset published', () => {
  const work = mkdtempSync(join(tmpdir(), 'screencomp-macos-release-'));
  try {
    const golden = JSON.parse(readFileSync(join(root, '.github/release-contract.json'), 'utf8'));
    const version = metadataVersion(), tag = `v${version}`;
    const arm64 = machoFixture(join(work, 'arm64-screencomp'), ARM64);
    const intel = machoFixture(join(work, 'x86_64-screencomp'), X86_64);
    const refusals = [
      [['x86_64-apple-darwin', tag, intel], /Intel macOS \(x86_64-apple-darwin\) is no longer released/],
      [['aarch64-apple-darwin', tag, intel], /Mach-O architecture mismatch/],
      [['aarch64-apple-darwin', 'v999.0.0', arm64], /tag does not match Cargo version/],
    ];
    for (const [[target, badTag, binary], reason] of refusals) {
      const refusedOut = join(work, 'refused');
      const got = pack(target, badTag, refusedOut, binary);
      assert.notEqual(got.status, 0); assert.match(got.stderr, reason);
      assert.ok(!existsSync(refusedOut), `${reason} must be refused before creating output`);
    }
    const intelBuildOut = join(work, 'intel-build');
    const intelBuild = spawnSync(process.execPath, [helper, 'x86_64-apple-darwin', tag, intelBuildOut], { cwd: root, encoding: 'utf8' });
    assert.notEqual(intelBuild.status, 0); assert.match(intelBuild.stderr, /Intel macOS \(x86_64-apple-darwin\) is no longer released/);
    assert.ok(!existsSync(intelBuildOut));

    const { jobs, producers } = releaseProducers();
    const artifactStore = join(work, 'forge-artifact-store'); mkdirSync(artifactStore);
    for (const producer of producers) {
      const artifactDir = join(artifactStore, producer.step.with.name.replace('${{ matrix.target }}', producer.matrix.target ?? ''));
      for (const target of producer.targets) {
        if (target.includes('darwin')) {
          const packed = pack(target, tag, artifactDir, arm64);
          assert.equal(packed.status, 0, packed.stderr);
          assert.equal(packed.stdout, `packaged screencomp-${tag}-${target}.tar.gz\n`);
          continue;
        }
        // Other lanes' artifacts are proved by their own native runs.
        mkdirSync(artifactDir, { recursive: true });
        const stem = golden.archiveStem.replace('$tag', tag).replace('$target', target);
        for (const ext of [target.includes('windows') ? golden.windowsExtension : golden.unixExtension, golden.checksumExtension]) writeFileSync(join(artifactDir, `${stem}.${ext}`), `forge transport payload for ${target}`);
      }
    }
    const { result, downloaded, published } = publishStaged({ jobs, artifactStore, work, tag, env: process.env });
    assert.equal(result.status, 0, result.stderr);
    const stem = golden.archiveStem.replace('$tag', tag).replace('$target', 'aarch64-apple-darwin');
    const archive = `${stem}.${golden.unixExtension}`, checksum = `${stem}.${golden.checksumExtension}`;
    assert.deepEqual(published.filter(name => /apple|darwin|macos/i.test(name)).sort(), [archive, checksum].sort());
    const archivePath = join(downloaded, archive);
    assert.equal(readFileSync(join(downloaded, checksum), 'utf8'), `${sha256(archivePath)}  ${archive}\n`);
    const unpacked = join(work, 'unpacked'); mkdirSync(unpacked);
    execFileSync('tar', ['-xzf', archivePath, '-C', unpacked]);
    assert.deepEqual(readdirSync(unpacked).sort(), [...golden.contents].sort());
    assert.deepEqual(readFileSync(join(unpacked, golden.binary)), readFileSync(arm64));
    const contract = JSON.parse(readFileSync(join(downloaded, `${stem}.contract.json`), 'utf8'));
    // Not installed or run on this host, so the contract records no installed hash.
    assert.deepEqual(contract, { schemaVersion: golden.schemaVersion, target: 'aarch64-apple-darwin', tag, version, archive, checksum, binarySha256: sha256(arm64), contents: golden.contents });
  } finally { rmSync(work, { recursive: true, force: true }); }
});

test('compiled-artifact boundary rejects truncated and incorrectly declared architectures before execution', () => {
  const dir = mkdtempSync(join(tmpdir(), 'screencomp-invalid-binary-'));
  const path = join(dir, 'binary');
  try {
    const golden = JSON.parse(readFileSync(join(root, '.github/release-contract.json'), 'utf8'));
    for (const target of golden.targets) {
      const cases = [[Buffer.alloc(4), /truncated/]];
      if (target.includes('linux')) {
        const elf = Buffer.alloc(64); elf.write('7f454c46', 0, 'hex'); elf[4] = 2; elf[5] = 1; elf.writeUInt16LE(target.startsWith('aarch64') ? 62 : 183, 18);
        cases.push([elf, /ELF architecture mismatch/]);
      } else if (target.includes('darwin')) {
        const macho = Buffer.alloc(32); macho.writeUInt32LE(0xfeedfacf, 0); macho.writeUInt32LE(target.startsWith('aarch64') ? 0x1000007 : 0x100000c, 4);
        cases.push([macho, /Mach-O architecture mismatch/]);
      } else {
        const outside = Buffer.alloc(64); outside.write('MZ'); outside.writeUInt32LE(0xffffffff, 0x3c);
        cases.push([outside, /PE header outside/]);
        const pe = Buffer.alloc(88); pe.write('MZ'); pe.writeUInt32LE(64, 0x3c); pe.writeUInt32LE(0x4550, 64); pe.writeUInt16LE(0xaa64, 68);
        cases.push([pe, /PE architecture mismatch/]);
      }
      for (const [bytes, reason] of cases) {
        writeFileSync(path, bytes);
        const got = spawnSync(process.execPath, [helper, 'verify-binary', target, path], { cwd: root, encoding: 'utf8' });
        assert.notEqual(got.status, 0); assert.match(got.stderr, reason); assert.doesNotMatch(got.stderr, /RangeError/);
      }
    }
  } finally { rmSync(dir, { recursive: true, force: true }); }
});

test('release entrypoint reports unreadable artifacts with corrective guidance', () => {
  const got = spawnSync(process.execPath, [helper, 'verify-binary', 'x86_64-unknown-linux-gnu', join(root, 'absent-release-binary')], { cwd: root, encoding: 'utf8' });
  assert.notEqual(got.status, 0);
  assert.match(got.stderr, /^release-artifact: cannot read compiled binary/);
  assert.match(got.stderr, /rerun the same release-artifact command before publishing/);
  assert.doesNotMatch(got.stderr, /at main/);
  const dir = mkdtempSync(join(tmpdir(), 'screencomp-blocked-output-'));
  try {
    const path = join(dir, 'file'); writeFileSync(path, 'occupied');
    const metadata = JSON.parse(execFileSync('cargo', ['metadata', '--no-deps', '--format-version', '1', '--locked'], { cwd: root, encoding: 'utf8' }));
    const version = metadata.packages.find(p => p.name === 'screencomp').version;
    const blocked = spawnSync(process.execPath, [helper, 'x86_64-unknown-linux-gnu', `v${version}`, join(path, 'release')], { cwd: root, encoding: 'utf8' });
    assert.notEqual(blocked.status, 0); assert.match(blocked.stderr, /release-artifact:.*ENOTDIR/);
    assert.match(blocked.stderr, /correct permissions\/content and rerun/); assert.doesNotMatch(blocked.stderr, /at main/);
  } finally { rmSync(dir, { recursive: true, force: true }); }
});

test('README platform lists match the release contract', () => {
  const golden = JSON.parse(readFileSync(join(root, '.github/release-contract.json'), 'utf8'));
  const readme = readFileSync(join(root, 'README.md'), 'utf8').replace(/\s+/g, ' ');
  const arches = suffix => ['x86_64', 'aarch64'].filter(arch => golden.targets.includes(`${arch}-${suffix}`)).map(arch => arch === 'aarch64' ? 'arm64' : arch);
  const mac = arches('apple-darwin');
  const macLabel = mac.includes('x86_64') ? 'macOS' : 'Apple Silicon macOS';
  const platforms = `Linux (${arches('unknown-linux-gnu').join(', ')}), ${macLabel} (${mac.join(', ')}), and Windows (${arches('pc-windows-msvc').join(', ')})`;
  // Both the install-script and release-binaries sections state the list.
  assert.equal(readme.split(platforms).length - 1, 2, `README must list exactly: ${platforms}`);
  const intelNotices = [/Intel macOS is no longer supported/, /refuses Intel macOS runners/];
  for (const notice of intelNotices) assert.equal(notice.test(readme), !mac.includes('x86_64'), `${notice} must match whether Intel macOS is released`);
});

// The installer and the install action detect the host with `uname` (and, on
// macOS, `sysctl` for Rosetta translation); those probes and the forge
// transport are the only doubles, so every journey runs the real scripts.
test('installers refuse Intel macOS and install the Apple Silicon artifact on arm64 Macs', { skip: process.platform === 'win32' && 'the uname/sysctl stand-ins need a POSIX PATH' }, () => {
  const dir = mkdtempSync(join(tmpdir(), 'screencomp-macos-install-'));
  try {
    const tag = `v${metadataVersion()}`;
    const assets = join(dir, 'assets');
    const fixture = machoFixture(join(dir, 'arm64-screencomp'), ARM64);
    const packed = pack('aarch64-apple-darwin', tag, assets, fixture);
    assert.equal(packed.status, 0, packed.stderr);
    const stem = `screencomp-${tag}-aarch64-apple-darwin`;
    const action = parse(readFileSync(join(root, 'action.yml'), 'utf8')).runs.steps.find(s => s.name === 'Install screencomp');
    const host = (name, { machine, translated }) => {
      const bin = join(dir, name, 'bin'); mkdirSync(bin, { recursive: true });
      const requests = join(dir, name, 'requests');
      writeFileSync(join(bin, 'uname'), `#!/bin/sh\ncase "$1" in -s) echo Darwin ;; -m) echo ${machine} ;; *) exit 2 ;; esac\n`);
      writeFileSync(join(bin, 'sysctl'), `#!/bin/sh\n[ "$*" = "-n sysctl.proc_translated" ] && echo ${translated}\n`);
      writeFileSync(join(bin, 'curl'), `#!/bin/sh\nset -eu\n[ "$1" = -fsSL ] && [ "$2" = -o ]\necho "$4" >> "${requests}"\ncp "${assets}/\${4##*/}" "$3"\n`);
      for (const tool of ['uname', 'sysctl', 'curl']) chmodSync(join(bin, tool), 0o755);
      const env = { ...process.env, PATH: `${bin}${process.platform === 'win32' ? ';' : ':'}${process.env.PATH}` };
      const installed = join(dir, name, 'installed'); const actionTemp = join(dir, name, 'runner'); mkdirSync(actionTemp);
      const script = spawnSync('bash', ['scripts/install.sh', '--version', tag, '--to', installed], { cwd: root, env, encoding: 'utf8' });
      const viaAction = spawnSync('bash', ['-c', action.run], { cwd: root, encoding: 'utf8', env: { ...env, SCREENCOMP_METHOD: 'download', SCREENCOMP_VERSION: tag, SCREENCOMP_REPO: 'nickderobertis/screencomp', SCREENCOMP_ACTION_PATH: root, RUNNER_TEMP: actionTemp, GITHUB_PATH: join(actionTemp, 'path'), GH_TOKEN: 'forge-double' } });
      return { script, viaAction, installed, actionTemp, requests: existsSync(requests) ? readFileSync(requests, 'utf8').trim().split('\n') : [] };
    };

    const intel = host('intel', { machine: 'x86_64', translated: 0 });
    for (const got of [intel.script, intel.viaAction]) {
      assert.notEqual(got.status, 0);
      assert.match(got.stderr, /Intel macOS is no longer supported/);
    }
    assert.deepEqual(intel.requests, [], 'Intel macOS must be refused before any asset download');
    assert.ok(!existsSync(join(intel.installed, 'screencomp')));

    const macho = readFileSync(fixture);
    for (const [name, probe] of [['apple-silicon', { machine: 'arm64', translated: 0 }], ['rosetta-shell', { machine: 'x86_64', translated: 1 }]]) {
      const mac = host(name, probe);
      assert.equal(mac.script.status, 0, mac.script.stderr);
      assert.deepEqual(readFileSync(join(mac.installed, 'screencomp')), macho);
      // The action extracts, then runs `--version`, which a Linux host cannot
      // do for a Mach-O binary; it must have fetched only the arm64 assets.
      assert.ok(existsSync(join(mac.actionTemp, 'screencomp-bin', 'screencomp')), mac.viaAction.stderr);
      assert.deepEqual(readFileSync(join(mac.actionTemp, 'screencomp-bin', 'screencomp')), macho);
      const base = `https://github.com/nickderobertis/screencomp/releases/download/${tag}`;
      const wanted = [`${base}/${stem}.tar.gz`, `${base}/${stem}.sha256`];
      assert.deepEqual(mac.requests, [...wanted, ...wanted], `${name}: script then action fetch the Apple Silicon assets only`);
      const verified = spawnSync(process.execPath, [helper, 'verify-binary', 'aarch64-apple-darwin', join(mac.installed, 'screencomp')], { cwd: root, encoding: 'utf8' });
      assert.equal(verified.status, 0, verified.stderr);
    }
  } finally { rmSync(dir, { recursive: true, force: true }); }
});
