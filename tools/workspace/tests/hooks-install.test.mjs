// Exercise the real installer, lefthook dispatcher and Git interfaces offline.
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { copyFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { root } from "./support.mjs";

const current = readFileSync(join(root, "lefthook.yml"), "utf8");
// Prior setup's duplicate commands, installed by the real hook manager.
const obsolete = `${current}\npre-push:\n  commands:\n    clippy:\n      run: cargo clippy --locked --workspace --all-targets --all-features -- -D warnings\n    test:\n      run: cargo nextest run --locked -p screencomp\n    llmlint-validate:\n      run: llmlint validate\n`;

function fixture(body) {
  const dir = mkdtempSync(join(process.env.ONEPIPELINE_NODE_SCRATCH_DIR ?? tmpdir(), "screencomp-hooks-"));
  const repo = join(dir, "clone");
  const remote = join(dir, "remote.git");
  const trace = join(dir, "trace.json");
  mkdirSync(repo);
  const env = { ...process.env, GIT_TRACE2_EVENT: trace };
  // Git advertises every repository-scoped variable a parent hook can export.
  const localEnv = spawnSync("git", ["rev-parse", "--local-env-vars"], { encoding: "utf8" });
  assert.equal(localEnv.status, 0, localEnv.stderr);
  for (const name of localEnv.stdout.trim().split("\n")) delete env[name];
  function run(bin, args, success = true) {
    const result = spawnSync(bin, args, { cwd: repo, env, encoding: "utf8" });
    assert.equal(result.error, undefined, `${bin}: ${result.error}`);
    assert.equal(result.status === 0, success, `${bin} ${args.join(" ")}: ${result.stdout}\n${result.stderr}`);
    return result;
  }
  try {
    run("git", ["init", "-q", "--initial-branch=main"]);
    run("git", ["init", "-q", "--bare", remote]);
    run("git", ["config", "user.name", "Hook journey"]);
    run("git", ["config", "user.email", "hooks@example.test"]);
    mkdirSync(join(repo, "scripts"));
    copyFileSync(join(root, "justfile"), join(repo, "justfile"));
    copyFileSync(join(root, "scripts/hooks-install.sh"), join(repo, "scripts/hooks-install.sh"));
    body({ repo, remote, trace, run });
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

for (const migrated of [false, true]) {
  test(`ordinary pushes retain commit checks without duplicate push checks (${migrated ? "existing clone" : "fresh setup"})`, () => fixture(({ repo, remote, trace, run }) => {
    if (migrated) {
      writeFileSync(join(repo, "lefthook.yml"), obsolete);
      run("lefthook", ["install"]);
      assert.ok(existsSync(join(repo, ".git/hooks/pre-push")));
    }
    writeFileSync(join(repo, "lefthook.yml"), current);
    run("just", ["hooks-install"]);
    assert.ok(!existsSync(join(repo, ".git/hooks/pre-push")));
    assert.ok(existsSync(join(repo, ".git/hooks/pre-commit")));
    assert.equal(run("git", ["config", "--get", "core.hooksPath"], false).status, 1);
    for (const artifact of ["shots/baseline/arm64.json", "shots/gallery/index.html"]) {
      mkdirSync(join(repo, artifact, ".."), { recursive: true });
      writeFileSync(join(repo, artifact), "unchanged visual artifact\n");
    }
    mkdirSync(join(repo, "public"));
    writeFileSync(join(repo, "public/screen.txt"), "first\n");
    run("git", ["add", "."]);
    const commit = run("git", ["commit", "-qm", "feat: first"]);
    assert.match(commit.stdout + commit.stderr, /conflicts/);
    run("git", ["remote", "add", "origin", remote]);
    function push() {
      run("git", ["push", "origin", "main"]);
      assert.equal(run("git", ["--git-dir", remote, "rev-parse", "refs/heads/main"]).stdout, run("git", ["rev-parse", "HEAD"]).stdout);
    }
    push();
    writeFileSync(join(repo, "public/screen.txt"), "changed   \n");
    run("git", ["add", "public/screen.txt"]);
    const rejected = run("git", ["commit", "-qm", "fix: whitespace"], false);
    assert.match(rejected.stdout + rejected.stderr, /trailing whitespace/);
    writeFileSync(join(repo, "public/screen.txt"), "changed\n");
    run("git", ["add", "public/screen.txt"]);
    run("git", ["commit", "-qm", "fix: changed"]);
    push();
    push();
    run("just", ["hooks-install"]);
    push();
    writeFileSync(join(repo, "public/screen.txt"), "after rerun   \n");
    run("git", ["add", "public/screen.txt"]);
    const rerunRejected = run("git", ["commit", "-qm", "fix: rerun whitespace"], false);
    assert.match(rerunRejected.stdout + rerunRejected.stderr, /trailing whitespace/);
    writeFileSync(join(repo, "public/screen.txt"), "after rerun\n");
    run("git", ["add", "public/screen.txt"]);
    run("git", ["commit", "-qm", "fix: after setup rerun"]);
    push();
    for (const artifact of ["shots/baseline/arm64.json", "shots/gallery/index.html"]) {
      assert.equal(readFileSync(join(repo, artifact), "utf8"), "unchanged visual artifact\n");
    }
    const children = readFileSync(trace, "utf8").trim().split("\n").map(JSON.parse)
      .filter(event => event.event === "child_start" || event.event === "start").map(event => event.argv.join(" "));
    assert.ok(children.some(argv => argv.includes("diff --cached --check")), children.join("\n"));
    assert.ok(!children.some(argv => /cargo (clippy|nextest)|llmlint validate|screencomp (manifest|gallery)|docker run/.test(argv)), children.join("\n"));
    const hookCalls = readFileSync(trace, "utf8").trim().split("\n").map(JSON.parse).filter(event => event.event === "child_start" && event.child_class === "hook");
    assert.ok(hookCalls.length > 0);
    assert.ok(hookCalls.every(event => event.hook_name === "pre-commit"), JSON.stringify(hookCalls));
    assert.equal(run("git", ["status", "--porcelain"]).stdout, "");
  }));
}

test("setup preserves a user-owned pre-push hook and shared hooksPath", () => fixture(({ repo, run }) => {
  writeFileSync(join(repo, "lefthook.yml"), current);
  run("git", ["config", "core.hooksPath", "local-hooks"]);
  mkdirSync(join(repo, "local-hooks"));
  const hook = join(repo, "local-hooks/pre-push");
  writeFileSync(hook, "#!/bin/sh\n# user-owned\nexit 0\n", { mode: 0o755 });
  run("just", ["hooks-install"], false);
  assert.equal(readFileSync(hook, "utf8"), "#!/bin/sh\n# user-owned\nexit 0\n");
  assert.equal(run("git", ["config", "--get", "core.hooksPath"]).stdout.trim(), "local-hooks");
}));

for (const modifiedDispatcher of [false, true]) {
  test(`successful setup preserves ${modifiedDispatcher ? "a modified dispatcher" : "a user-owned hook"} in the default hooks directory`, () => fixture(({ repo, run }) => {
    const hook = join(repo, ".git/hooks/pre-push");
    if (modifiedDispatcher) {
      writeFileSync(join(repo, "lefthook.yml"), obsolete);
      run("lefthook", ["install"]);
      writeFileSync(hook, `${readFileSync(hook, "utf8")}\n# user customization\n`);
    } else {
      writeFileSync(hook, "#!/bin/sh\n# independent user hook\nexit 0\n", { mode: 0o755 });
    }
    const before = readFileSync(hook, "utf8");
    writeFileSync(join(repo, "lefthook.yml"), current);
    run("just", ["hooks-install"]);
    assert.equal(readFileSync(hook, "utf8"), before);
    assert.ok(existsSync(join(repo, ".git/hooks/pre-commit")));
  }));
}
