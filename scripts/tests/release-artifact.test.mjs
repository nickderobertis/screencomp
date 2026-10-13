import assert from 'node:assert/strict';
import { execFileSync, spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { cpSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { parse } from 'yaml';
import { chmodSync } from 'node:fs';
import { test } from 'node:test';
const root = resolve(import.meta.dirname, '../..');
const helper = join(root, 'scripts/release-artifact.mjs');
const pkgName = metadata => metadata.packages.find(p => p.name === 'screencomp').name;
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
    const workflow = parse(readFileSync(join(root, '.github/workflows/release.yml'), 'utf8'));
    const jobs = workflow.jobs;
    const artifactStore = join(out, 'forge-artifact-store'); mkdirSync(artifactStore);
    const producers = [
      ...jobs.build.strategy.matrix.include.map(row => ({ targets: [row.target], step: jobs.build.steps.find(s => s.uses === 'actions/upload-artifact@v4'), matrix: row })),
      { targets: jobs['build-darwin'].env.DARWIN_TARGETS.split(/\s+/), step: jobs['build-darwin'].steps.find(s => s.uses === 'actions/upload-artifact@v4'), matrix: {} },
    ];
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
        for (const file of [archiveName, sumName, `${stem}.contract.json`]) {
          if (publishedTarget === target) cpSync(join(out, file), join(artifactDir, file));
          else writeFileSync(join(artifactDir, file), `forge transport payload for ${publishedTarget}`);
        }
        wanted.push(archiveName, sumName);
      }
    }
    const download = jobs.upload.steps.find(s => s.uses === 'actions/download-artifact@v4');
    assert.equal(download.with['merge-multiple'], true);
    assert.match(download.with.pattern, /^[A-Za-z0-9_-]+\*$/);
    const publishingRoot = join(out, 'linux-publication'); const downloaded = join(publishingRoot, download.with.path);
    mkdirSync(downloaded, { recursive: true });
    const prefix = download.with.pattern.slice(0, -1);
    // The forge artifact service is the doubled boundary; the upload command
    // runs exactly as production with the download action's actual selection.
    for (const artifact of readdirSync(artifactStore).filter(name => name.startsWith(prefix))) for (const file of readdirSync(join(artifactStore, artifact))) cpSync(join(artifactStore, artifact, file), join(downloaded, file));
    const request = join(out, 'forge-upload.json');
    writeFileSync(join(transport, 'gh'), '#!/usr/bin/env node\nconst fs = require("node:fs"); const args = process.argv.slice(2); if (process.env.FORGE_FAIL === "true") { console.error("forge upload unavailable"); process.exit(1); } for (const file of args.slice(3)) if (!fs.existsSync(file)) process.exit(1); fs.writeFileSync(process.env.FORGE_REQUEST, JSON.stringify({args, repo:process.env.GH_REPO}));\n');
    chmodSync(join(transport, 'gh'), 0o755);
    const publish = jobs.upload.steps.find(s => s.name === 'Publish verified archives and checksums');
    const uploaded = spawnSync('bash', ['-c', publish.run], { cwd: publishingRoot, encoding: 'utf8', env: { ...actionEnv, TAG: tag, GH_TOKEN: 'forge-double', GH_REPO: 'nickderobertis/screencomp', FORGE_REQUEST: request } });
    assert.equal(uploaded.status, 0, uploaded.stderr);
    const uploadRequest = JSON.parse(readFileSync(request, 'utf8'));
    assert.deepEqual(uploadRequest.args.slice(0, 3), ['release', 'upload', tag]);
    assert.equal(uploadRequest.repo, 'nickderobertis/screencomp');
    assert.deepEqual(uploadRequest.args.slice(3).map(path => path.replaceAll('\\', '/').split('/').at(-1)).sort(), wanted.sort());
    assert.ok(uploadRequest.args.slice(3).every(path => !path.endsWith('.contract.json')));
    const failedUpload = spawnSync('bash', ['-c', publish.run], { cwd: publishingRoot, encoding: 'utf8', env: { ...actionEnv, TAG: tag, GH_TOKEN: 'forge-double', GH_REPO: 'nickderobertis/screencomp', FORGE_REQUEST: request, FORGE_FAIL: 'true' } });
    assert.notEqual(failedUpload.status, 0); assert.match(failedUpload.stderr, /forge upload unavailable/);



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
