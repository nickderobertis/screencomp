// The demo project's `browser-test` recipe (`just _demo-browser-test`): it must
// hand the demo's capture spec a scratch SHOTS_OUT, fail with the next step when
// the spec writes no captures.json, and remove the scratch either way. npm and
// npx are stood in here: the real run installs from the npm registry and drives
// Chromium, which this offline tier has neither of (`just test-browser all demo`
// is that run).
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { chmodSync, existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { test } from "node:test";

import { root } from "./support.mjs";

function runRecipe(writesCaptures, npmStatus = 0) {
  const bin = mkdtempSync(join(tmpdir(), "screencomp-demo-bin-"));
  try {
    const record = join(bin, "record");
    writeFileSync(
      join(bin, "npm"),
      `#!/bin/sh\necho "npm $*" >> "${record}"\necho "stand-in npm output: exit ${npmStatus}"\nexit ${npmStatus}\n`,
    );
    writeFileSync(
      join(bin, "npx"),
      `#!/bin/sh\necho "npx $* SHOTS_OUT=$SHOTS_OUT cwd=$(pwd)" >> "${record}"\n` +
        (writesCaptures ? `echo '{"schema":1,"shots":[]}' > "$SHOTS_OUT/captures.json"\n` : ""),
    );
    chmodSync(join(bin, "npm"), 0o755);
    chmodSync(join(bin, "npx"), 0o755);
    const run = spawnSync("just", ["_demo-browser-test"], {
      cwd: root,
      encoding: "utf8",
      env: { ...process.env, PATH: `${bin}:${process.env.PATH}` },
    });
    const calls = readFileSync(record, "utf8").trim().split("\n");
    const shotsOut = calls[1] && /SHOTS_OUT=(\S+)/.exec(calls[1])[1];
    return { run, calls, shotsOut };
  } finally {
    rmSync(bin, { recursive: true, force: true });
  }
}

test("the demo spec runs in demo/ from its own lockfile, into a scratch it removes", () => {
  const { run, calls, shotsOut } = runRecipe(true);
  assert.equal(run.status, 0, run.stderr);
  assert.equal(calls[0], "npm ci --no-audit --no-fund");
  assert.doesNotMatch(run.stdout + run.stderr, /stand-in npm output/, "a successful install is quiet");
  const [, cwd] = /^npx playwright test SHOTS_OUT=\S+ cwd=(.+)$/.exec(calls[1]);
  // resolve(): bash on Windows prints `D:/…`, the same directory as `D:\…`.
  assert.equal(resolve(cwd), join(root, "demo"));
  assert.ok(!existsSync(shotsOut), "the scratch SHOTS_OUT is removed");
});

test("a spec that writes no captures.json fails with the next step, and the scratch still goes", () => {
  const { run, shotsOut } = runRecipe(false);
  assert.notEqual(run.status, 0);
  assert.match(run.stderr, /the demo capture wrote no captures\.json; ACTION: check demo\/tests\/screenshots\.spec\.ts/);
  assert.ok(!existsSync(shotsOut), "the scratch SHOTS_OUT is removed on failure too");
});

test("a failed demo install shows npm's own output and the next step, and runs no spec", () => {
  const { run, calls } = runRecipe(true, 1);
  assert.notEqual(run.status, 0);
  assert.match(run.stderr, /stand-in npm output: exit 1/);
  assert.match(run.stderr, /the demo's 'npm ci' failed; its output is above\. ACTION: /);
  assert.deepEqual(calls, ["npm ci --no-audit --no-fund"]);
});
