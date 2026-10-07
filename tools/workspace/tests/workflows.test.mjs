// Contracts over the committed workflows, read through a YAML parser: the fixed
// status-check contexts every pull request must report, the gate's tier hand-off,
// and the review-only, PR-title and llmlint jobs' shapes.
import assert from "node:assert/strict";
import { spawn, spawnSync } from "node:child_process";
import { createServer } from "node:http";
import { chmodSync, mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { parse } from "yaml";

import { root } from "./support.mjs";

const dir = join(root, ".github/workflows");
const workflows = Object.fromEntries(
  readdirSync(dir)
    .filter((file) => /\.ya?ml$/.test(file))
    .map((file) => [file, parse(readFileSync(join(dir, file), "utf8"))]),
);

/** The fixed contexts branch protection is reconciled against (gov-screencomp). */
const REQUIRED = [
  "check (ubuntu-latest)",
  "check (macos-latest)",
  "check (windows-latest)",
  "workflows",
  "action",
  "action-download",
];

/** Whether a workflow runs on every pull request: no branch, path or type filter that could skip one. */
function runsOnEveryPullRequest(workflow) {
  const on = workflow.on;
  if (on === "pull_request" || (Array.isArray(on) && on.includes("pull_request"))) return true;
  if (!on || typeof on !== "object" || !("pull_request" in on)) return false;
  const filter = on.pull_request ?? {};
  const skips = ["branches", "branches-ignore", "paths", "paths-ignore"].some((key) => key in filter);
  const types = filter.types ?? ["opened", "synchronize", "reopened"];
  return !skips && ["opened", "synchronize", "reopened"].every((type) => types.includes(type));
}

/** The status-check contexts a job reports, matrix legs expanded the way GitHub names them. */
function contexts(key, job) {
  const name = job.name ?? key;
  const matrix = job.strategy?.matrix;
  if (!matrix || job.name) return [name];
  const axes = Object.entries(matrix).filter(([axis]) => !["include", "exclude"].includes(axis));
  assert.equal(axes.length, 1, `${key}: one matrix axis`);
  return axes[0][1].map((value) => `${name} (${value})`);
}

test("every fixed context is reported on every pull request, unconditionally", () => {
  const found = new Map();
  for (const [file, workflow] of Object.entries(workflows)) {
    if (!runsOnEveryPullRequest(workflow)) continue;
    for (const [key, job] of Object.entries(workflow.jobs)) {
      for (const context of contexts(key, job)) found.set(context, { file, key, job, workflow });
    }
  }
  for (const context of REQUIRED) {
    const at = found.get(context);
    assert.ok(at, `no job reports ${context} on every pull request`);
    assert.equal(at.file, "ci.yml", context);
    assert.equal(at.job.if, undefined, `${context} has an if: that could leave it unreported`);
    for (const need of [at.job.needs ?? []].flat()) {
      assert.equal(at.workflow.jobs[need].if, undefined, `${context} needs ${need}, which can be skipped`);
    }
  }
  const check = workflows["ci.yml"].jobs.check;
  assert.deepEqual(check.strategy.matrix.os, ["ubuntu-latest", "macos-latest", "windows-latest"]);
  assert.equal(check.strategy["fail-fast"], false);
});

test("the check jobs run the tier ci-tier.mjs selects, so a red sweep fails them", () => {
  const steps = workflows["ci.yml"].jobs.check.steps;
  assert.equal(steps[0].with?.["fetch-depth"], 0, "the merge base needs full history");
  const tier = steps.findIndex((step) => step.id === "tier");
  const gate = steps.findIndex((step) => step.name === "Check");
  assert.ok(tier >= 0 && gate > tier);
  assert.equal(steps[tier].run, 'node tools/workspace/ci-tier.mjs >> "$GITHUB_OUTPUT"');
  assert.equal(steps[gate].run, 'just check "$TIER"');
  assert.equal(steps[gate].env.TIER, "${{ steps.tier.outputs.tier }}");
  assert.equal(steps[gate].env.NX_BASE, "${{ steps.tier.outputs.base }}");
  for (const step of steps) {
    assert.equal(step.if, undefined, `${step.name ?? step.uses}: conditional`);
    assert.ok(!step["continue-on-error"], `${step.name ?? step.uses}: continue-on-error`);
  }
});

test("notignored is a review comment of its own, needed by no job and in no fixed context", () => {
  const workflow = workflows["notignored.yml"];
  assert.ok(runsOnEveryPullRequest(workflow));
  assert.deepEqual(workflow.permissions, { contents: "read", "pull-requests": "write" });
  const jobs = Object.entries(workflow.jobs);
  assert.equal(jobs.length, 1);
  const [key, job] = jobs[0];
  assert.equal(job.if, "github.event.pull_request.head.repo.full_name == github.repository");
  assert.ok(job.steps.some((step) => step.uses === "nickderobertis/notignored@v0"));
  assert.equal(job.steps[0].with["fetch-depth"], 0);
  assert.ok(!REQUIRED.includes(job.name ?? key));
  for (const [file, other] of Object.entries(workflows)) {
    for (const [name, needer] of Object.entries(other.jobs ?? {})) {
      if (file === "notignored.yml") continue;
      assert.ok(![needer.needs ?? []].flat().includes(key), `${file}:${name} needs ${key}`);
    }
  }
});

/**
 * The PR-title action's own verdict on `title`: its built entry point (the file
 * the runner executes, installed from the same tag the workflow uses) run with
 * the workflow step's `with:` inputs, against a local stand-in for the one REST
 * call it makes, which serves an open pull request carrying that title.
 */
async function actionVerdict(inputs, title) {
  const server = createServer((_req, res) => {
    res.setHeader("content-type", "application/json");
    res.end(JSON.stringify({ number: 1, title, labels: [] }));
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const scratch = mkdtempSync(join(tmpdir(), "pr-title-"));
  try {
    const event = join(scratch, "event.json");
    writeFileSync(event, JSON.stringify({ pull_request: { number: 1, base: { user: { login: "o" }, repo: { name: "r" } } } }));
    writeFileSync(join(scratch, "output"), "");
    // The host's environment (Windows sockets need SystemRoot), minus any
    // runner context or action input a CI host would otherwise leak in.
    const host = Object.entries(process.env).filter(([key]) => !/^(GITHUB|INPUT)_/i.test(key));
    const env = {
      ...Object.fromEntries(host),
      GITHUB_TOKEN: "unused",
      GITHUB_EVENT_NAME: "pull_request",
      GITHUB_EVENT_PATH: event,
      GITHUB_OUTPUT: join(scratch, "output"),
      INPUT_GITHUBBASEURL: `http://127.0.0.1:${server.address().port}`,
    };
    for (const [key, value] of Object.entries(inputs)) env[`INPUT_${key.toUpperCase()}`] = value;
    const entry = join(root, "node_modules/action-semantic-pull-request/dist/index.js");
    const child = spawn(process.execPath, [entry], { env, stdio: ["ignore", "pipe", "pipe"] });
    let stdout = "";
    child.stdout.on("data", (chunk) => (stdout += chunk));
    const status = await new Promise((resolve) => child.on("close", resolve));
    return { status, stdout };
  } finally {
    server.close();
    rmSync(scratch, { recursive: true, force: true });
  }
}

test("pr-title admits exactly release-plz's commit types, on every title-changing event", async () => {
  const workflow = workflows["pr-title.yml"];
  assert.deepEqual(workflow.on.pull_request.types, ["opened", "edited", "synchronize", "reopened", "ready_for_review"]);
  assert.deepEqual(workflow.permissions, { "pull-requests": "read" });
  const job = workflow.jobs["pr-title"];
  assert.ok(job, "a job named pr-title");
  assert.equal(job.permissions, undefined, "the job inherits read-only pull-requests");
  const step = job.steps.find((s) => s.uses?.startsWith("amannn/action-semantic-pull-request@"));
  assert.equal(step.uses, "amannn/action-semantic-pull-request@v6");
  const types = step.with.types.trim().split(/\s+/);

  const releasePlz = readFileSync(join(root, "release-plz.toml"), "utf8");
  const parsed = [...releasePlz.matchAll(/\{ message = "\^([a-z]+)"/g)].map((match) => match[1]);
  assert.deepEqual([...types].sort(), [...parsed].sort());
});

test("the PR-title action, as the workflow runs it, accepts releasable titles and rejects malformed ones", async () => {
  const step = workflows["pr-title.yml"].jobs["pr-title"].steps.find((s) => s.uses?.startsWith("amannn/"));
  // The installed copy comes from the repository and tag the workflow names;
  // bun.lock records the commit that tag resolved to when it was installed.
  // llmlint: ignore[contracts_have_one_source_or_a_drift_gate] pr-title.yml floats on the major tag `@v6` by design, as the repository's other third-party actions float on theirs, and this suite is offline by rule, so no test here can resolve what that tag points to today; the tag spelling is the one source both sides share, and this test holds them to it.
  const installed = JSON.parse(readFileSync(join(root, "package.json"), "utf8")).devDependencies["action-semantic-pull-request"];
  assert.equal(installed, `github:${step.uses.replace("@", "#")}`);

  for (const title of ["feat: add x", "fix(cli): handle y", "feat!: drop z", "chore: release v1.2.3"]) {
    const { status, stdout } = await actionVerdict(step.with, title);
    assert.equal(status, 0, `accepts ${title}: ${stdout}`);
  }
  for (const [title, reason] of [
    ["Add x", /No release type found/],
    ["feature: add x", /Unknown release type "feature"/],
    ["feat add x", /No release type found/],
    ["feat:", /No release type found/],
    ["Fix: y", /Unknown release type "Fix"/],
  ]) {
    const { status, stdout } = await actionVerdict(step.with, title);
    assert.equal(status, 1, `rejects ${title}: ${stdout}`);
    assert.match(stdout, reason, title);
  }
});

test("llmlint validates, then requires the credential, then always runs the judged diff", () => {
  const workflow = workflows["llmlint.yml"];
  assert.ok(runsOnEveryPullRequest(workflow));
  const job = workflow.jobs.llmlint;
  assert.equal(job.if, undefined);
  const runs = job.steps.map((step) => step.run ?? "");
  const validate = runs.indexOf("just lint-llm-validate --diff-base origin/main");
  const credential = runs.indexOf("bash scripts/llmlint-require-credential.sh");
  const judged = runs.indexOf("just lint-llm-diff origin/main");
  assert.ok(validate >= 0 && credential > validate && judged > credential, runs.join(" | "));
  for (const index of [credential, judged]) {
    assert.equal(job.steps[index].env.OPENAI_API_KEY, "${{ secrets.OPENAI_API_KEY }}");
  }
  for (const step of job.steps) {
    assert.equal(step.if, undefined, `${step.name ?? step.uses}: a condition could skip the judged step`);
    assert.ok(!step["continue-on-error"], `${step.name ?? step.uses}: continue-on-error`);
  }
  assert.ok(runs.some((run) => /npm install --global "@openai\/codex@/.test(run)), "installs codex");
  const harnesses = /^harnesses = \[(.*)\]$/m.exec(readFileSync(join(root, "oneharness.toml"), "utf8"))[1];
  assert.match(harnesses, /^"codex"/, "codex is the harness oneharness.toml tries first");
});

test("the credential check fails fast without OPENAI_API_KEY or when codex rejects it, and authenticates codex with it", () => {
  const bin = mkdtempSync(join(tmpdir(), "screencomp-codex-"));
  try {
    const record = join(bin, "login");
    writeFileSync(
      join(bin, "codex"),
      `#!/bin/sh\necho "$*" > "${record}"\ncat >> "${record}"\n[ "$(tail -n 1 "${record}")" != sk-revoked ]\n`,
    );
    chmodSync(join(bin, "codex"), 0o755);
    const run = (env) =>
      spawnSync("bash", ["scripts/llmlint-require-credential.sh"], {
        cwd: root,
        encoding: "utf8",
        env: { PATH: `${bin}:${process.env.PATH}`, ...env },
      });
    const missing = run({});
    assert.equal(missing.status, 1);
    assert.match(missing.stderr, /OPENAI_API_KEY/);
    const present = run({ OPENAI_API_KEY: "sk-test" });
    assert.equal(present.status, 0, present.stderr);
    assert.equal(readFileSync(record, "utf8"), "login --with-api-key\nsk-test\n");
    const rejected = run({ OPENAI_API_KEY: "sk-revoked" });
    assert.equal(rejected.status, 1);
    assert.match(rejected.stderr, /codex rejected the OPENAI_API_KEY repository secret; replace it/);
  } finally {
    rmSync(bin, { recursive: true, force: true });
  }
});
