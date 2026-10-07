// The gate's two entry points into the JavaScript toolchain, run as the recipes
// run them: node-modules.sh (the locked install, healed when stale) against
// stand-ins for the package managers it calls, and the `nx` wrapper against the
// real Nx in a scratch copy of the tree.
//
// bun and npm are stood in because a real install resolves packages from the
// npm registry, and this tier is offline. The real install is the one every
// gate run starts from: each recipe reaches Nx through this script, so a tree
// whose package.json, bun.lock and pin do not compose fails the gate itself.
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import {
  chmodSync,
  cpSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { test } from "node:test";

import { removeScratch, root, scratchCopy } from "./support.mjs";

const PIN = /"packageManager": *"bun@([0-9.]+)"/.exec(readFileSync(join(root, "package.json"), "utf8"))[1];

/**
 * A directory holding node-modules.sh, package.json and bun.lock, plus a `bin`
 * of stand-ins: `bun` reporting `bunVersion` (absent when null) and `npm`, each
 * appending its argv to `calls` and exiting `status`.
 */
function installFixture({ bunVersion, status = 0, nodeOnPath = true, npmOnPath = true }) {
  const dir = mkdtempSync(join(tmpdir(), "screencomp-node-modules-"));
  for (const file of ["tools/workspace/node-modules.sh", "package.json", "bun.lock", "browser-tests/package.json"]) {
    mkdirSync(dirname(join(dir, file)), { recursive: true });
    cpSync(join(root, file), join(dir, file));
  }
  const bin = join(dir, "bin");
  mkdirSync(bin);
  const calls = join(dir, "calls");
  const standIn = (name, body) => {
    writeFileSync(join(bin, name), `#!/bin/sh\n${body}`);
    chmodSync(join(bin, name), 0o755);
  };
  // The install a stand-in performs: what `bun install` leaves behind.
  const install = `mkdir -p node_modules/nx && echo '{}' > node_modules/nx/package.json\nexit ${status}\n`;
  if (bunVersion !== null) {
    standIn("bun", `if [ "$1" = "--version" ]; then echo ${bunVersion}; exit 0; fi\necho "bun $*" >> "${calls}"\n${install}`);
  }
  if (npmOnPath) standIn("npm", `echo "npm $*" >> "${calls}"\n${install}`);
  if (nodeOnPath) symlinkSync(process.execPath, join(bin, "node"));
  const run = () =>
    spawnSync("bash", [join(dir, "tools/workspace/node-modules.sh")], {
      encoding: "utf8",
      env: { PATH: `${bin}:/usr/bin:/bin`, HOME: dir },
    });
  const recorded = () => (existsSync(calls) ? readFileSync(calls, "utf8").trim().split("\n") : []);
  return { dir, run, recorded, stamp: join(dir, "node_modules/.bun-lock-installed") };
}

test("a stale install runs the pinned bun's frozen install and stamps it", () => {
  const fx = installFixture({ bunVersion: PIN });
  try {
    const run = fx.run();
    assert.equal(run.status, 0, run.stderr);
    assert.deepEqual(fx.recorded(), ["bun install --frozen-lockfile --silent"]);
    assert.ok(readFileSync(fx.stamp, "utf8").startsWith(readFileSync(join(fx.dir, "bun.lock"), "utf8")));
    const again = fx.run();
    assert.equal(again.status, 0);
    assert.equal(fx.recorded().length, 1, "a matching stamp installs nothing");
  } finally {
    rmSync(fx.dir, { recursive: true, force: true });
  }
});

for (const file of ["bun.lock", "package.json", "browser-tests/package.json"]) {
  test(`${file} changed since the stamped install reinstalls and restamps`, () => {
    const fx = installFixture({ bunVersion: PIN });
    try {
      assert.equal(fx.run().status, 0);
      const before = readFileSync(fx.stamp, "utf8");
      writeFileSync(join(fx.dir, file), `${readFileSync(join(fx.dir, file), "utf8")}\n`);
      const run = fx.run();
      assert.equal(run.status, 0, run.stderr);
      assert.equal(fx.recorded().length, 2, `a changed ${file} reinstalls`);
      assert.notEqual(readFileSync(fx.stamp, "utf8"), before);
      assert.equal(fx.run().status, 0);
      assert.equal(fx.recorded().length, 2, "and the new stamp holds");
    } finally {
      rmSync(fx.dir, { recursive: true, force: true });
    }
  });
}

test("a pin removed after a stamped install is still refused", () => {
  const fx = installFixture({ bunVersion: PIN });
  try {
    assert.equal(fx.run().status, 0);
    const path = join(fx.dir, "package.json");
    writeFileSync(path, readFileSync(path, "utf8").replace(/"packageManager": *"[^"]*",?/, ""));
    const run = fx.run();
    assert.equal(run.status, 1);
    assert.match(run.stderr, /pins no bun version/);
  } finally {
    rmSync(fx.dir, { recursive: true, force: true });
  }
});

test("a package.json pinning no bun fails with the next step", () => {
  const fx = installFixture({ bunVersion: PIN });
  try {
    const path = join(fx.dir, "package.json");
    writeFileSync(path, readFileSync(path, "utf8").replace(/"packageManager": *"[^"]*",?/, ""));
    const run = fx.run();
    assert.equal(run.status, 1);
    assert.match(run.stderr, /pins no bun version/);
    assert.match(run.stderr, /ACTION: /);
    assert.deepEqual(fx.recorded(), []);
  } finally {
    rmSync(fx.dir, { recursive: true, force: true });
  }
});

for (const workspaces of ['"browser-tests/*"', '["../outside"]', '[1]', '"browser-tests"']) {
  test(`a workspaces list of ${workspaces} is refused before any install`, () => {
    const fx = installFixture({ bunVersion: PIN });
    try {
      const path = join(fx.dir, "package.json");
      writeFileSync(path, readFileSync(path, "utf8").replace(/"workspaces": *\[[^\]]*\]/, `"workspaces": ${workspaces}`));
      const run = fx.run();
      assert.equal(run.status, 1);
      assert.match(run.stderr, /cannot read package\.json's workspaces: .*"workspaces" must be a list of plain relative directories/);
      assert.deepEqual(fx.recorded(), []);
    } finally {
      rmSync(fx.dir, { recursive: true, force: true });
    }
  });
}

test("neither the pinned bun nor npm fails with the next step", () => {
  const fx = installFixture({ bunVersion: "0.0.1", npmOnPath: false });
  try {
    const run = fx.run();
    assert.equal(run.status, 1);
    assert.match(run.stderr, new RegExp(`bun ${PIN.replaceAll(".", "\\.")} .* is not on PATH, and there is no npm`));
    assert.deepEqual(fx.recorded(), []);
  } finally {
    rmSync(fx.dir, { recursive: true, force: true });
  }
});

test("a machine with no bun installs the pinned one through npm", () => {
  const fx = installFixture({ bunVersion: null });
  try {
    const run = fx.run();
    assert.equal(run.status, 0, run.stderr);
    assert.deepEqual(fx.recorded(), [`npm exec --yes --package=bun@${PIN} -- bun install --frozen-lockfile --silent`]);
    assert.ok(existsSync(fx.stamp));
  } finally {
    rmSync(fx.dir, { recursive: true, force: true });
  }
});

test("a bun at another version is bypassed for the pinned one", () => {
  const fx = installFixture({ bunVersion: "0.0.1" });
  try {
    const run = fx.run();
    assert.equal(run.status, 0, run.stderr);
    assert.deepEqual(fx.recorded(), [`npm exec --yes --package=bun@${PIN} -- bun install --frozen-lockfile --silent`]);
  } finally {
    rmSync(fx.dir, { recursive: true, force: true });
  }
});

test("a failed install fails with the next step and leaves no stamp", () => {
  const fx = installFixture({ bunVersion: PIN, status: 1 });
  try {
    const run = fx.run();
    assert.equal(run.status, 1);
    assert.match(run.stderr, /'bun install --frozen-lockfile' \(bun [0-9.]+\) failed/);
    assert.match(run.stderr, /ACTION: /);
    assert.ok(!existsSync(fx.stamp));
  } finally {
    rmSync(fx.dir, { recursive: true, force: true });
  }
});

test("no Node.js fails with the next step", (t) => {
  if (spawnSync("sh", ["-c", "command -v node"], { env: { PATH: "/usr/bin:/bin" } }).status === 0) {
    t.skip("this machine has a system Node.js in /usr/bin, so it cannot be taken off PATH here");
    return;
  }
  const fx = installFixture({ bunVersion: PIN, nodeOnPath: false });
  try {
    const run = fx.run();
    assert.equal(run.status, 1);
    assert.match(run.stderr, /Node\.js is not installed/);
    assert.deepEqual(fx.recorded(), []);
  } finally {
    rmSync(fx.dir, { recursive: true, force: true });
  }
});

test("the nx wrapper is one line on success, the whole log on failure, raw on request", () => {
  const dir = scratchCopy();
  try {
    const nx = (args, env = {}) =>
      spawnSync("bash", ["tools/workspace/nx", ...args], { cwd: dir, encoding: "utf8", env: { ...process.env, ...env } });

    const ok = nx(["run", "demo:lint"]);
    assert.equal(ok.status, 0, ok.stderr);
    const lines = ok.stdout.trim().split("\n");
    assert.equal(lines.length, 1, ok.stdout);
    assert.match(lines[0], /^nx run demo:lint: .*\(log: \.nx\/logs\/nx\.[A-Za-z0-9]+\)$/);

    const failed = nx(["run", "no-such-project:lint"]);
    assert.equal(failed.status, 1);
    assert.match(failed.stderr, /no-such-project/);
    assert.match(failed.stderr, /nx run no-such-project:lint: FAILED/);

    const raw = nx(["show", "projects", "--json"], { NX_SHOW_OUTPUT: "1" });
    assert.equal(raw.status, 0, raw.stderr);
    assert.ok(JSON.parse(raw.stdout).includes("screencomp"));
  } finally {
    removeScratch(dir);
  }
});
