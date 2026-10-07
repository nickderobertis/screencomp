// The Claude Code SessionStart hook (scripts/session-setup.sh), run exactly as
// .claude/settings.json runs it, against a scratch copy of the tree whose
// setup-llmlint.sh is replaced by a stand-in that records being reached. Every
// path a session start takes past the CI and opt-out exits must hand off to it,
// return promptly with exit 0 whether it succeeds, fails or hangs, and leave the
// session's own output unchanged; those two exits must not hand off at all.
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { chmodSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";

import { removeScratch, scratchCopy } from "./support.mjs";

const STAND_INS = {
  succeeds: "touch .dev/llmlint-reached\nexit 0\n",
  fails: "touch .dev/llmlint-reached\nexit 1\n",
  hangs: "touch .dev/llmlint-reached\nsleep 8\n",
};

function withScratch(setupLlmlint, body) {
  const dir = scratchCopy();
  try {
    if (setupLlmlint !== null) {
      writeFileSync(join(dir, "scripts/setup-llmlint.sh"), `#!/usr/bin/env bash\n${setupLlmlint}`);
      chmodSync(join(dir, "scripts/setup-llmlint.sh"), 0o755);
    }
    // The opt-in provisioning path launches the full machine setup; stand it in too.
    writeFileSync(join(dir, "scripts/setup.sh"), "#!/usr/bin/env bash\nexit 0\n");
    body(dir);
  } finally {
    removeScratch(dir);
  }
}

/** Run the hook as the SessionStart command does, timing it. */
function hook(dir, env = {}) {
  const started = Date.now();
  const clean = Object.fromEntries(
    Object.entries(process.env).filter(
      ([key]) => !["GITHUB_ACTIONS", "CI", "SCREENCOMP_SKIP_SETUP", "SCREENCOMP_AUTO_SETUP"].includes(key),
    ),
  );
  const run = spawnSync("bash", [join(dir, "scripts/session-setup.sh")], {
    cwd: dir,
    encoding: "utf8",
    env: { ...clean, CLAUDE_PROJECT_DIR: dir, ...env },
    timeout: 20_000,
  });
  return { ...run, seconds: (Date.now() - started) / 1000 };
}

/** Wait (bounded) for the detached hand-off to record itself. */
function reached(dir, waitMs = 10_000) {
  const marker = join(dir, ".dev/llmlint-reached");
  const until = Date.now() + waitMs;
  while (Date.now() < until) {
    if (existsSync(marker)) return true;
    spawnSync("sleep", ["0.1"]);
  }
  return existsSync(marker);
}

/** Mark the scratch copy's environment as already set up (the silent path). */
function markReady(dir) {
  const run = spawnSync("bash", ["-c", ". scripts/setup-lib.sh && _load_tool_env && _write_stamp && _check_ready"], {
    cwd: dir,
    encoding: "utf8",
  });
  return run.status === 0;
}

for (const [outcome, script] of Object.entries(STAND_INS)) {
  test(`the not-yet-set-up path hands off and returns promptly when setup-llmlint ${outcome}`, () => {
    withScratch(script, (dir) => {
      const run = hook(dir);
      assert.equal(run.status, 0, run.stderr);
      assert.ok(run.seconds < 5, `took ${run.seconds}s`);
      assert.match(run.stdout, /Dev environment not set up yet|Dev environment not ready/);
      assert.ok(reached(dir), "setup-llmlint.sh was not reached");
    });
  });
}

test("the opt-in provisioning path hands off too", () => {
  withScratch(STAND_INS.fails, (dir) => {
    const run = hook(dir, { SCREENCOMP_AUTO_SETUP: "1" });
    assert.equal(run.status, 0, run.stderr);
    assert.ok(run.seconds < 5, `took ${run.seconds}s`);
    assert.match(run.stdout, /provisioning in the BACKGROUND/);
    assert.ok(reached(dir));
  });
});

test("the already-set-up path stays silent and hands off", (t) => {
  withScratch(STAND_INS.hangs, (dir) => {
    if (!markReady(dir)) {
      t.skip("this machine lacks setup-lib's REQUIRED_BINS, so the ready path cannot be reached here");
      return;
    }
    const run = hook(dir);
    assert.equal(run.status, 0, run.stderr);
    assert.equal(run.stdout, "");
    assert.ok(run.seconds < 5, `took ${run.seconds}s`);
    assert.ok(reached(dir));
  });
});

test("the real setup-llmlint.sh with no uv on PATH neither blocks nor fails the session", () => {
  withScratch(null, (dir) => {
    const home = mkdtempSync(join(tmpdir(), "screencomp-home-"));
    try {
      // No uv anywhere the hook or the installer looks: not on PATH, not in the
      // installer's ~/.local/bin, and no asdf shim directory to resolve one from.
      const run = hook(dir, { HOME: home, PATH: "/usr/bin:/bin", ASDF_DATA_DIR: join(home, ".asdf") });
      assert.equal(run.status, 0, run.stderr);
      assert.ok(run.seconds < 5, `took ${run.seconds}s`);
      const log = join(dir, ".dev/setup-llmlint.log");
      const until = Date.now() + 10_000;
      while (Date.now() < until && !/uv not found/.test(existsSync(log) ? readFileSync(log, "utf8") : "")) {
        spawnSync("sleep", ["0.1"]);
      }
      assert.match(readFileSync(log, "utf8"), /uv not found; cannot install llmlint/);
    } finally {
      rmSync(home, { recursive: true, force: true });
    }
  });
});

for (const [why, env] of [
  ["CI", { GITHUB_ACTIONS: "true" }],
  ["the opt-out", { SCREENCOMP_SKIP_SETUP: "1" }],
]) {
  test(`${why} exits early without handing off`, () => {
    withScratch(STAND_INS.succeeds, (dir) => {
      mkdirSync(join(dir, ".dev"), { recursive: true });
      const run = hook(dir, env);
      assert.equal(run.status, 0, run.stderr);
      assert.equal(run.stdout, "");
      assert.ok(!reached(dir, 2_000), "setup-llmlint.sh ran on an early-exit path");
    });
  });
}
