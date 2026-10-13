import { lstatSync, readFileSync, readlinkSync, readdirSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parse } from 'yaml';
import { Lexer, Parser, Evaluator, data } from '@actions/expressions';
export const root = resolve(fileURLToPath(new URL('..', import.meta.url)));
const supportedPattern = p => typeof p === 'string' && !/[?+\[\]{}!\\]/.test(p) && !/\*{3,}/.test(p);
export function validateWorkflow(file, workflow) {
  if (!workflow || typeof workflow !== 'object' || !workflow.jobs || typeof workflow.jobs !== 'object' || Array.isArray(workflow.jobs) || !Object.keys(workflow.jobs).length) throw new Error(`invalid workflow object ${file}`);
  const on = typeof workflow.on === 'string' ? { [workflow.on]: null } : Array.isArray(workflow.on) ? Object.fromEntries(workflow.on.map(e => [e, null])) : workflow.on;
  if (!on || typeof on !== 'object') throw new Error(`missing event declarations ${file}`);
  const supported = {
    push: ['branches', 'branches-ignore', 'tags', 'tags-ignore', 'paths', 'paths-ignore'],
    pull_request: ['types', 'branches', 'branches-ignore', 'paths', 'paths-ignore'],
    release: ['types'], workflow_run: ['workflows', 'types', 'branches', 'branches-ignore'],
    repository_dispatch: ['types'], schedule: ['cron'], workflow_dispatch: ['inputs'], workflow_call: ['inputs', 'outputs', 'secrets'],
  };
  for (const [event, filter] of Object.entries(on)) {
    if (!(event in supported)) throw new Error(`unsupported event ${file}:${event}`);
    if (event === 'schedule') {
      if (!Array.isArray(filter) || !filter.every(f => f && typeof f.cron === 'string' && Object.keys(f).length === 1)) throw new Error(`unsupported schedule ${file}`);
      continue;
    }
    if (filter === null) continue;
    if (!filter || typeof filter !== 'object' || Array.isArray(filter)) throw new Error(`invalid event filter ${file}:${event}`);
    for (const [key, value] of Object.entries(filter)) {
      if (!supported[event].includes(key)) throw new Error(`unsupported trigger filter ${file}:${event}:${key}`);
      if (['inputs', 'outputs', 'secrets'].includes(key)) {
        const object = v => v !== null && typeof v === 'object' && !Array.isArray(v);
        if (!object(value) || !Object.values(value).every(object)) throw new Error(`invalid declarations ${file}:${event}:${key}`);
        for (const [name, definition] of Object.entries(value)) {
          if (definition.description !== undefined && typeof definition.description !== 'string') throw new Error(`invalid declaration description ${file}:${name}`);
          if (definition.required !== undefined && typeof definition.required !== 'boolean') throw new Error(`invalid declaration required ${file}:${name}`);
          if (key === 'inputs') {
            const types = event === 'workflow_call' ? ['boolean', 'number', 'string'] : ['boolean', 'number', 'string', 'choice', 'environment'];
            if (!types.includes(definition.type)) throw new Error(`invalid input type ${file}:${name}`);
            const expected = ['choice', 'environment'].includes(definition.type) ? 'string' : definition.type;
            if (definition.default !== undefined && typeof definition.default !== expected) throw new Error(`invalid input default ${file}:${name}`);
          }
          if (key === 'outputs' && typeof definition.value !== 'string') throw new Error(`invalid output value ${file}:${name}`);
        }
      }
      if (!['inputs', 'outputs', 'secrets'].includes(key) && (!Array.isArray(value) || !value.every(v => supportedPattern(v)))) throw new Error(`unsupported trigger filter values ${file}:${event}:${key}`);
    }
  }
  for (const [id, job] of Object.entries(workflow.jobs)) {
    if (!job || typeof job !== 'object' || Array.isArray(job) || (!job.uses && !job['runs-on'])) throw new Error(`invalid job ${file}:${id}`);
    if (job.if !== undefined && !['string', 'boolean'].includes(typeof job.if)) throw new Error(`invalid condition ${file}:${id}`);
    if (job.needs !== undefined && ![job.needs].flat().every(n => typeof n === 'string')) throw new Error(`invalid needs ${file}:${id}`);
  }
  return workflow;
}
export function workflows() {
  return Object.fromEntries(readdirSync(join(root, '.github/workflows')).filter(f => /\.ya?ml$/.test(f)).sort().map(f => [f, validateWorkflow(f, parse(readFileSync(join(root, '.github/workflows', f), 'utf8')))]));
}
export function sourceDigest() {
  const hash = createHash('sha256');
  const files = execFileSync('git', ['ls-files', '-z', '--cached', '--others', '--exclude-standard'], { cwd: root, encoding: 'utf8' }).split('\0').filter(Boolean).sort();
  for (const path of files) {
    const absolute = join(root, path);
    hash.update(path).update('\0').update(lstatSync(absolute).isSymbolicLink() ? readlinkSync(absolute) : readFileSync(absolute)).update('\0');
  }
  return hash.digest('hex');
}
const datum = value => {
  if (value === null || value === undefined) return new data.Null();
  if (typeof value === 'boolean') return new data.BooleanData(value);
  if (typeof value === 'number') return new data.NumberData(value);
  if (typeof value === 'string') return new data.StringData(value);
  if (Array.isArray(value)) return new data.Array(...value.map(datum));
  return new data.Dictionary(...Object.entries(value).map(([key, value]) => ({ key, value: datum(value) })));
};
export function evaluate(source, context, results = []) {
  if (typeof source === 'boolean') return source;
  if (typeof source !== 'string') throw new Error('invalid Actions expression');
  const expression = source.trim().replace(/^\$\{\{\s*|\s*\}\}$/g, '');
  const functions = new Map([
    ['always', { name: 'always', minArgs: 0, maxArgs: 0, call: () => datum(true) }],
    ['success', { name: 'success', minArgs: 0, maxArgs: 0, call: () => datum(results.every(r => r === 'success')) }],
    ['failure', { name: 'failure', minArgs: 0, maxArgs: 0, call: () => datum(results.includes('failure')) }],
    ['cancelled', { name: 'cancelled', minArgs: 0, maxArgs: 0, call: () => datum(results.includes('cancelled')) }],
  ]);
  const ast = new Parser(new Lexer(expression).lex().tokens, Object.keys(context), [...functions.values()]).parse();
  return JSON.parse(JSON.stringify(new Evaluator(ast, datum(context), functions).evaluate(), data.replacer));
}
export function interpolate(value, context) {
  if (typeof value !== 'string') return value;
  if (/^\$\{\{.*\}\}$/.test(value)) return evaluate(value, context);
  return value.replace(/\$\{\{.*?\}\}/g, expr => String(evaluate(expr, context)));
}
export function matrices(matrix, context) {
  if (matrix === undefined) return [{}];
  if (matrix === null || matrix === false || matrix === 0 || matrix === '') throw new Error('unresolved matrix object');
  if (typeof matrix === 'string') matrix = evaluate(matrix, context);
  if (!matrix || typeof matrix !== 'object' || Array.isArray(matrix)) throw new Error('unresolved matrix object');
  const axes = Object.entries(matrix).filter(([key]) => !['include', 'exclude'].includes(key));
  let rows = [{}];
  for (const [key, raw] of axes) {
    const values = interpolate(raw, context);
    if (!Array.isArray(values)) throw new Error(`unresolved matrix axis ${key}`);
    rows = rows.flatMap(row => values.map(value => ({ ...row, [key]: value })));
  }
  const exclude = interpolate(matrix.exclude ?? [], context);
  if (!Array.isArray(exclude) || !exclude.every(row => row && typeof row === 'object' && !Array.isArray(row))) throw new Error('invalid matrix exclude rows');
  rows = rows.filter(row => !exclude.some(ex => Object.entries(ex).every(([k,v]) => row[k] === v)));
  const include = interpolate(matrix.include ?? [], context);
  if (!Array.isArray(include) || !include.every(row => row && typeof row === 'object' && !Array.isArray(row))) throw new Error('unresolved matrix include rows');
  if (!axes.length) return include.length ? include : rows;
  const added = [];
  for (const inc of include) {
    let merged = false;
    for (const row of rows) if (axes.every(([k]) => !(k in inc) || row[k] === inc[k])) { Object.assign(row, inc); merged = true; }
    if (!merged) added.push(inc);
  }
  return [...rows, ...added];
}
function glob(path, pattern) {
  if (!supportedPattern(pattern)) throw new Error(`unsupported trigger glob ${pattern}`);
  let regex = '';
  for (let i = 0; i < pattern.length;) {
    if (pattern.startsWith('**/', i)) { regex += '(?:.*/)?'; i += 3; }
    else if (pattern.startsWith('**', i)) { regex += '.*'; i += 2; }
    else if (pattern[i] === '*') { regex += '[^/]*'; i++; }
    else { regex += pattern[i].replace(/[.+^$()|[\]\\]/g, '\\$&'); i++; }
  }
  return typeof path === 'string' && new RegExp(`^${regex}$`).test(path);
}
export function triggered(workflow, scenario) {
  let on = workflow.on;
  if (typeof on === 'string') on = { [on]: {} };
  if (Array.isArray(on)) on = Object.fromEntries(on.map(e => [e, {}]));
  if (!on || !(scenario.eventName in on)) return false;
  const filter = on[scenario.eventName] ?? {};
  const types = filter.types ?? (scenario.eventName === 'pull_request' ? ['opened', 'synchronize', 'reopened'] : null);
  if (types && !types.includes(scenario.event.action)) return false;
  const branch = scenario.eventName === 'pull_request' ? scenario.event.pull_request?.base?.ref : scenario.eventName === 'workflow_run' ? scenario.event.workflow_run?.head_branch : scenario.event.ref?.replace('refs/heads/', '');
  const tag = scenario.event.ref?.startsWith('refs/tags/') ? scenario.event.ref.slice(10) : null;
  if ((filter.tags || filter['tags-ignore']) && !tag && !(filter.branches || filter['branches-ignore'])) return false;
  if (tag && (filter.branches || filter['branches-ignore']) && !(filter.tags || filter['tags-ignore'])) return false;
  if (tag && filter.tags && !filter.tags.some(p => glob(tag, p))) return false;
  if (filter['tags-ignore']?.some(p => glob(tag, p))) return false;
  if (!tag && filter.branches && !filter.branches.some(p => glob(branch ?? '', p))) return false;
  if (!tag && filter['branches-ignore']?.some(p => glob(branch ?? '', p))) return false;
  if (!tag && filter.paths && !scenario.paths.some(p => filter.paths.some(g => glob(p, g)))) return false;
  if (!tag && filter['paths-ignore'] && scenario.paths.every(p => filter['paths-ignore'].some(g => glob(p, g)))) return false;
  if (filter.workflows && !filter.workflows.some(p => glob(scenario.event.workflow_run?.name, p))) return false;
  return true;
}
export function releaseTargets(graph = workflows()) {
  const jobs = graph['release.yml'].jobs;
  return [...jobs.build.strategy.matrix.include.map(r => r.target), ...jobs['build-darwin'].env.DARWIN_TARGETS.split(/\s+/)].sort();
}
export function resolveRun(file, scenario, graph = workflows(), inputs = {}, stack = []) {
  if (stack.includes(file)) throw new Error(`recursive workflow call: ${file}`);
  const workflow = graph[file];
  if (!workflow) throw new Error(`unresolved workflow call: ${file}`);
  inputs = { ...Object.fromEntries(Object.entries(workflow.on?.workflow_call?.inputs ?? {}).map(([key, v]) => [key, v.default ?? ''])), ...scenario.inputs, ...inputs };
  const results = Object.create(null);
  const rows = [];
  const pending = new Map(Object.entries(workflow.jobs));
  while (pending.size) {
    let progress = false;
    for (const [id, job] of pending) {
      const needsIds = [job.needs ?? []].flat();
      if (needsIds.some(n => !results[n])) continue;
      const needs = Object.fromEntries(needsIds.map(n => [n, results[n]]));
      const github = { event_name: scenario.eventName, event: scenario.event, repository: 'nickderobertis/screencomp', ref: scenario.event.ref ?? 'refs/pull/1/merge', sha: scenario.head, actor: 'nickderobertis' };
      const context = { github, needs, inputs, vars: { PUBLISH_TO_CRATES_IO: scenario.publish ? 'true' : 'false' }, matrix: {} };
      const outcomes = needsIds.map(n => results[n].result);
      const condition = job.if ?? 'success()';
      const explicitStatus = /\b(always|success|failure|cancelled)\(/.test(condition);
      const enabled = (explicitStatus || outcomes.every(r => r === 'success')) && Boolean(evaluate(condition, context, outcomes));
      let outputs = {};
      if (id === 'policy') {
        const stepOutputs = Object.fromEntries(['macosRequired', 'isReleaseOnly'].map(k => [k, String(scenario.classification[k])]));
        const outputContext = { ...context, steps: { policy: { outputs: stepOutputs } } };
        outputs = Object.fromEntries(Object.entries(job.outputs ?? {}).map(([key, expression]) => [key, String(interpolate(expression, outputContext))]));
      }
      if (id === 'arches') {
        const arches = JSON.parse(inputs.arches || 'null');
        if (!Array.isArray(arches) || !arches.length || !arches.every(arch => ['x86_64', 'arm64', 'aarch64'].includes(arch))) throw new Error('unresolved capture arches; caller needs an explicit architecture fixture');
        const projects = JSON.parse(inputs.projects || '[]');
        const safePath = value => typeof value === 'string' && value.length > 0 && !value.startsWith('/') && !value.includes('\\') && value.split('/').every(p => p && p !== '.' && p !== '..');
        if (!Array.isArray(projects) || !projects.every(p => p && typeof p === 'object' && typeof p.id === 'string' && /^[A-Za-z0-9_-]+$/.test(p.id) && ['current', 'verify'].every(k => !(k in p) || (safePath(p[k]) && p[k].startsWith('shots/'))) && (!('manifest' in p) || safePath(p.manifest)) && (!('gallery-title' in p) || (typeof p['gallery-title'] === 'string' && p['gallery-title'].length > 0))) || new Set(projects.map(p => p.id)).size !== projects.length) throw new Error('invalid capture project fields');
        outputs.matrix = JSON.stringify({ include: arches.flatMap(arch => (projects.length ? projects : [{ id: '', current: 'shots/current', verify: 'shots/verify' }]).map(p => ({ arch, project: p.id, current: p.current ?? `shots/current/${p.id}`, verify: p.verify ?? `shots/verify/${p.id}`, manifest: p.manifest ?? (p.id === '' ? '' : `shots/baseline/${p.id}/${arch}.json`), gallery_title: p['gallery-title'] ?? inputs['gallery-title'] }))) });
      }
      results[id] = { result: enabled ? 'success' : 'skipped', outputs };
      for (const matrix of matrices(job.strategy?.matrix, context)) {
        context.matrix = matrix;
        if (job.uses) {
          const match = /^(?:\.\/|nickderobertis\/screencomp\/)(\.github\/workflows\/[^@]+)(?:@[^\s]+)?$/.exec(job.uses);
          if (!match) throw new Error(`unresolved workflow call: ${job.uses}`);
          if (!graph[match[1].split('/').at(-1)]) throw new Error(`unresolved workflow call: ${job.uses}`);
          if (!enabled) continue;
          const childInputs = Object.fromEntries(Object.entries(job.with ?? {}).map(([k,v]) => [k, interpolate(v, context)]));
          rows.push(...resolveRun(match[1].split('/').at(-1), scenario, graph, childInputs, [...stack, file]).rows);
        } else {
          const runner = interpolate(job['runs-on'], context);
          if (typeof runner !== 'string' || !/^(ubuntu-|windows-|macos-)/.test(runner)) throw new Error(`unresolved runner ${file}:${id}: ${JSON.stringify(runner)}`);
          const name = interpolate(job.name ?? id, context);
          const displayName = !job.name && Object.keys(matrix).length ? `${name} (${Object.values(matrix).join(', ')})` : name;
          rows.push({ workflow: file, job: id, displayName, runner, matrix, targets: job.env?.DARWIN_TARGETS?.split(/\s+/) ?? (matrix.target ? [matrix.target] : []), event: scenario.eventName, enabled, macos: enabled && runner.startsWith('macos-'), head: scenario.head, tag: scenario.event.release?.tag_name ?? null, needs: needsIds });
        }
      }
      pending.delete(id); progress = true;
    }
    if (!progress) throw new Error(`unresolved needs/cycle in ${file}: ${[...pending.keys()]}`);
  }
  return { rows, results };
}
export function resolveEvent(scenario, graph = workflows()) {
  return Object.entries(graph).filter(([,workflow]) => triggered(workflow, scenario)).map(([workflow]) => ({ workflow, ...resolveRun(workflow, scenario, graph) }));
}

// Follow declared workflow_run listeners for each exact-head completion. Reusable
// jobs remain inside their caller's run; a downstream trigger creates a new run.
export function resolveFanout(scenario, graph = workflows()) {
  const runs = resolveEvent(scenario, graph);
  for (let i = 0; i < runs.length; i++) {
    const completed = runs[i];
    if (completed.ancestry?.includes(completed.workflow)) throw new Error(`workflow_run cycle at ${completed.workflow}`);
    const downstream = {
      ...scenario,
      eventName: 'workflow_run',
      event: { action: 'completed', repository: scenario.event.repository, workflow_run: { name: graph[completed.workflow].name, head_sha: scenario.head, conclusion: 'success', head_branch: scenario.event.pull_request?.head?.ref ?? scenario.event.ref?.replace('refs/heads/', '') } },
      inputs: {},
      classification: { isReleaseOnly: false, macosRequired: false, reason: 'automatic downstream event' },
    };
    for (const run of resolveEvent(downstream, graph)) runs.push({ ...run, triggeredBy: completed.workflow, ancestry: [...(completed.ancestry ?? []), completed.workflow] });
  }
  return runs;
}
