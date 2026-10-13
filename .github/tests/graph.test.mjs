import assert from 'node:assert/strict';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { test } from 'node:test';
import { history, prEvent } from './history.mjs';
import { evaluate, matrices, releaseTargets, resolveEvent, resolveFanout, resolveRun, root, sourceDigest, workflows } from '../workflow-graph.mjs';
const required = ['action', 'action-download', 'check (macos-latest)', 'check (ubuntu-latest)', 'check (windows-latest)', 'llmlint', 'pr-title', 'workflows'];
const classes = JSON.parse(readFileSync(join(root, '.github/event-classes.json'), 'utf8')).classes;
test('actual workflow graph resolves every event/diff/ownership route and records lifecycle telemetry', () => {
  const h = history(); const graph = workflows(); const scenarios = [];
  try {
    for (const klass of classes) for (const kind of Object.keys(h.heads)) for (const fork of [false, true]) for (const publish of [false, true]) for (const branch of ['fix/ordinary', 'release-plz-2026']) {
      const head = h.heads[kind];
      const event = klass.eventName === 'pull_request' ? prEvent({ fork, branch, action: klass.action, base: h.base, head }) : { action: klass.action, ref: klass.ref, before: h.base, repository: { full_name: 'nickderobertis/screencomp' }, release: { tag_name: 'v0.4.10', target_commitish: head }, workflow_run: { name: 'Release', head_sha: head, conclusion: 'success' } };
      const classification = h.classify(event, head, { eventName: klass.eventName });
      const scenario = { id: `${klass.id}/${kind}/${fork ? 'fork' : 'same'}/${publish}/${branch}`, class: klass.id, eventName: klass.eventName, event, head, diff: kind, fork, branch, publish, inputs: { 'macos-diagnostic': klass.diagnostic ?? false }, classification, paths: kind === 'source' ? ['src/lib.rs'] : kind === 'version-only' ? ['Cargo.toml', 'Cargo.lock', 'CHANGELOG.md'] : ['Cargo.toml'] };
      const runs = resolveFanout(scenario, graph);
      for (const run of runs) for (const row of run.rows) if (row.macos) {
        assert.ok((row.workflow === 'ci.yml' && (scenario.eventName === 'pull_request' || klass.diagnostic)) || (row.workflow === 'release.yml' && scenario.eventName === 'release'), `${scenario.id}: forbidden Mac route ${JSON.stringify(row)}`);
      }
      if (klass.eventName === 'pull_request') {
        const names = runs.flatMap(r => r.rows.map(row => row.displayName));
        for (const name of required) assert.equal(names.filter(n => n === name).length, 1, `${scenario.id}: required context ${name}`);
        const mac = runs.find(r => r.workflow === 'ci.yml').rows.find(row => row.job === 'check-macos');
        assert.equal(mac.enabled, classification.macosRequired);
        assert.equal(runs.find(r => r.workflow === 'ci.yml').results['macos-context'].result, 'success');
      }
      if (klass.eventName === 'workflow_dispatch') {
        const mac = runs.find(r => r.workflow === 'ci.yml').rows.find(row => row.job === 'check-macos');
        assert.equal(mac.enabled, klass.diagnostic ?? false);
      }
      scenarios.push({ ...scenario, runs });
    }
    // Exercise the unchanged public reusable call for every released CPU family
    // and optional publish combination, using its actual capture/report matrices.
    const arches = [...new Set(releaseTargets(graph).map(t => t.startsWith('aarch64') ? 'arm64' : 'x86_64'))];
    for (const arch of arches) for (const publish of [false, true]) {
      const scenario = scenarios.find(s => s.class === 'pr-synchronize' && !s.fork && s.publish === publish);
      const run = resolveRun('visual-docs-reusable.yml', scenario, graph, { arches: JSON.stringify([arch]), publish, pages: publish });
      assert.ok(run.rows.every(row => !row.macos));
      scenarios.push({ id: `reusable/${arch}/${publish}`, class: 'reusable-call', runs: [{ workflow: 'visual-docs-reusable.yml', ...run }] });
    }
    const lifecycles = [];
    for (const prClass of ['pr-opened', 'pr-synchronize', 'pr-reopened']) for (const kind of Object.keys(h.heads)) for (const fork of [false, true]) for (const publish of [false, true]) {
      const select = (klass, diff = kind, owner = fork, branch = 'fix/ordinary') => scenarios.find(s => s.class === klass && s.diff === diff && s.fork === owner && s.publish === publish && s.branch === branch);
      const events = [select(prClass), select('base-push'), select('pr-synchronize', 'version-only', false, 'release-plz-2026'), select('release-pr-merge', 'version-only', false), select('release-tag-push'), select('release-published'), select('downstream-smoke'), select('repository-dispatch')];
      const spendingRuns = events.flatMap(s => s.runs.filter(r => r.rows.some(row => row.macos)).map(r => ({ event: s.class, scenario: s.id, workflow: r.workflow, jobs: r.rows.filter(row => row.macos) })));
      lifecycles.push({ id: `${prClass}/${kind}/${fork}/${publish}`, events: events.map(s => s.id), spendingRuns });
    }
    const jobsPerRun = Math.max(...scenarios.flatMap(s => s.runs.map(r => r.rows.filter(row => row.macos).length)));
    const runsPerChange = Math.max(...lifecycles.map(l => l.spendingRuns.length));
    mkdirSync(join(root, '.github/.telemetry'), { recursive: true });
    writeFileSync(join(root, '.github/.telemetry/macos-policy.json'), JSON.stringify({ schemaVersion: 1, sourceDigest: sourceDigest(), releaseTargets: releaseTargets(graph), jobsPerRun, runsPerChange, scenarios, lifecycles }, null, 2) + '\n');
  } finally { h.clean(); }
});
test('unsupported expressions, runners, reusable calls and missing needs fail explicitly', () => {
  assert.throws(() => evaluate('unknown.context', {}), /Unrecognized named-value/);
  const scenario = { eventName: 'push', event: {}, head: 'exact-head', classification: {}, paths: [] };
  for (const job of [{ 'runs-on': '${{ inputs.unresolved }}' }, { uses: 'other/repo/.github/workflows/a.yml@main' }, { 'runs-on': 'ubuntu-latest', needs: 'missing' }]) assert.throws(() => resolveRun('test.yml', scenario, { 'test.yml': { jobs: { job } } }), /unresolved/);
  assert.deepEqual(matrices({ os: ['linux', 'mac'], arch: ['x64', 'arm'], exclude: [{ os: 'mac', arch: 'arm' }], include: [{ os: 'mac', arch: 'arm', extra: true }] }, {}), [{ os: 'linux', arch: 'x64' }, { os: 'linux', arch: 'arm' }, { os: 'mac', arch: 'x64' }, { os: 'mac', arch: 'arm', extra: true }]);
});
test('release graph preserves every platform/artifact and gates Linux publication on both builders', () => {
  const graph = workflows(); const contract = JSON.parse(readFileSync(join(root, '.github/release-contract.json'), 'utf8'));
  assert.deepEqual(releaseTargets(graph), contract.targets);
  const jobs = graph['release.yml'].jobs;
  assert.deepEqual(jobs.upload.needs, ['build', 'build-darwin']);
  assert.equal(jobs.upload['runs-on'], 'ubuntu-latest');
  for (const id of ['build', 'build-darwin']) {
    assert.equal(jobs[id].needs, 'test');
    assert.equal(jobs[id].steps[0].with.ref, '${{ github.event.release.tag_name }}');
    assert.ok(jobs[id].steps.some(s => s.run?.includes('node scripts/release-artifact.mjs')));
    assert.equal(jobs[id].permissions, undefined);
    assert.ok(jobs[id].steps.some(s => s.uses === 'actions/upload-artifact@v4' && s.with['if-no-files-found'] === 'error'));
  }
  assert.equal(jobs['build-darwin']['runs-on'], 'macos-15');
  assert.match(jobs['build-darwin'].steps.find(s => s.name?.startsWith('Provision')).run, /--install-rosetta/);
  assert.match(jobs.upload.steps.at(-1).run, /\*\.tar\.gz.*\*\.zip.*\*\.sha256/);
  for (const id of ['publish-crate', 'advance-major-tag']) assert.deepEqual(jobs[id].needs, ['upload']);
  const helper = readFileSync(join(root, 'scripts/release-artifact.mjs'), 'utf8');
  assert.match(helper, /arch', \['-x86_64', 'bash'/);
  assert.match(helper, /arch', \['-x86_64', installedBinary/);
  assert.match(helper, /hash\(binary\) !== hash\(installedBinary\)/);
  for (const doc of contract.contents.slice(1)) assert.ok(helper.includes(doc));
});

test("fan-out follows actual workflow_run names and keeps the exact artifact head", () => {
  const scenario = { eventName: "push", event: { ref: "refs/heads/main" }, head: "exact-head", classification: { macosRequired: false }, paths: [] };
  const graph = {
    "build.yml": { name: "Builder", on: { push: null }, jobs: { build: { "runs-on": "ubuntu-latest" } } },
    "smoke.yml": { name: "Smoke", on: { workflow_run: { workflows: ["Builder"], types: ["completed"] } }, jobs: { install: { "runs-on": "ubuntu-latest" } } },
  };
  const runs = resolveFanout(scenario, graph);
  assert.deepEqual(runs.map(r => r.workflow), ["build.yml", "smoke.yml"]);
  assert.equal(runs[1].triggeredBy, "build.yml");
  assert.equal(runs[1].rows[0].head, scenario.head);
  graph["build.yml"].on.workflow_run = { workflows: ["Smoke"] };
  assert.throws(() => resolveFanout(scenario, graph), /cycle/);
});
