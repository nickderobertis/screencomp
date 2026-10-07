// Every Nx run the tooling starts nested inside another Nx task (the boundary
// check, the tests' `nx`/`affectedBy`, the `nx` wrapper) takes nx-env.mjs's
// environment, so Nx's default plugins load in-process rather than in workers
// that exit when their load message misses a fixed 10s window on a loaded host.
// A scratch copy registers a probe plugin that records which process loaded it,
// and the host environment asks for isolation, as a CI runner's may.
import assert from "node:assert/strict";
import { spawn, spawnSync } from "node:child_process";
import { existsSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";

import { NX_ENV, nxBin } from "../nx-env.mjs";
import { affectedBy, nx, removeScratch, root, scratchCopy } from "./support.mjs";

const PROBE = `const { appendFileSync } = require("node:fs");
appendFileSync(process.env.SCREENCOMP_PLUGIN_PROBE, process.argv[1] + "\\n");
exports.name = "screencomp-probe";
exports.createNodesV2 = ["**/no-such-file.probe", () => []];
`;

/** A scratch copy whose nx.json registers the probe, and the file it records into. */
function withProbe(body) {
  const dir = scratchCopy();
  try {
    writeFileSync(join(dir, "tools/probe-plugin.js"), PROBE);
    const path = join(dir, "nx.json");
    writeFileSync(path, JSON.stringify({ ...JSON.parse(readFileSync(path, "utf8")), plugins: ["./tools/probe-plugin.js"] }));
    const log = join(dir, "probe.log");
    const saved = { ...process.env };
    Object.assign(process.env, { SCREENCOMP_PLUGIN_PROBE: log, NX_ISOLATE_PLUGINS: "true", NX_PLUGIN_NO_TIMEOUTS: "false" });
    try {
      body(dir, () => (existsSync(log) ? readFileSync(log, "utf8").trim().split("\n") : []));
    } finally {
      for (const key of ["SCREENCOMP_PLUGIN_PROBE", "NX_ISOLATE_PLUGINS", "NX_PLUGIN_NO_TIMEOUTS"]) {
        if (key in saved) process.env[key] = saved[key];
        else delete process.env[key];
      }
    }
  } finally {
    removeScratch(dir);
  }
}

const inWorker = (loads) => loads.filter((argv1) => /plugin-worker\.js$/.test(argv1));

test("the probe sees a worker when Nx runs outside nx-env.mjs with isolation asked for", () => {
  withProbe((dir, loads) => {
    const run = spawnSync(process.execPath, [nxBin(root), "show", "projects", "--json"], {
      cwd: dir,
      encoding: "utf8",
      env: { ...process.env, NX_DAEMON: "false", NX_NO_CLOUD: "true" },
    });
    assert.equal(run.status, 0, run.stderr);
    assert.ok(inWorker(loads()).length > 0, `loaded by: ${loads().join(", ")}`);
  });
});

test("every nested Nx entry point loads plugins in-process", () => {
  withProbe((dir, loads) => {
    const entries = {
      "the tests' nx()": () => nx(dir, ["show", "projects", "--json"]),
      "the tests' affectedBy()": () => affectedBy(["src/main.rs"], dir),
      "the boundary check": () => {
        const run = spawnSync(process.execPath, ["tools/workspace/check-project-boundaries.mjs", "workspace"], {
          cwd: dir,
          encoding: "utf8",
        });
        assert.equal(run.status, 0, run.stderr);
      },
      "the nx wrapper": () => {
        const run = spawnSync("bash", ["tools/workspace/nx", "show", "projects", "--json"], {
          cwd: dir,
          encoding: "utf8",
          env: { ...process.env, NX_SHOW_OUTPUT: "1" },
        });
        assert.equal(run.status, 0, run.stderr);
      },
    };
    for (const [entry, run] of Object.entries(entries)) {
      const before = loads().length;
      run();
      const now = loads().slice(before);
      assert.ok(now.length > 0, `${entry}: the probe plugin never loaded`);
      assert.deepEqual(inWorker(now), [], `${entry} loaded a plugin in an isolated worker`);
    }
  });
});

test("nx-env.mjs is the only place the tooling resolves Nx, and it pins plugin loading", () => {
  assert.equal(NX_ENV.NX_ISOLATE_PLUGINS, "false");
  assert.equal(NX_ENV.NX_PLUGIN_NO_TIMEOUTS, "true");
  const dir = join(root, "tools/workspace");
  const scripts = [dir, join(dir, "tests")].flatMap((d) => readdirSync(d).map((f) => join(d, f)));
  const resolvers = scripts.filter((f) => f.endsWith(".mjs") && /\bbin\.nx\b/.test(readFileSync(f, "utf8")));
  assert.deepEqual(
    resolvers.map((f) => f.slice(root.length + 1).replaceAll("\\", "/")),
    ["tools/workspace/nx-env.mjs"],
  );
});

test("several nested graph computations at once all pass", async () => {
  const runs = Array.from({ length: 6 }, () => {
    const child = spawn(process.execPath, ["tools/workspace/check-project-boundaries.mjs", "workspace"], {
      cwd: root,
      stdio: ["ignore", "pipe", "pipe"],
    });
    let output = "";
    child.stdout.on("data", (chunk) => (output += chunk));
    child.stderr.on("data", (chunk) => (output += chunk));
    return new Promise((resolve) => child.on("close", (status) => resolve({ status, output })));
  });
  for (const { status, output } of await Promise.all(runs)) assert.equal(status, 0, output);
});
