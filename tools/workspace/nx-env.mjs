// The one place every Nx run in this workspace takes its environment from: the
// `nx` wrapper exports it (it prints `KEY=VALUE` lines when run directly), and
// the boundary check and the tooling's tests run Nx through `runNx` below.
//
// Most of those runs are nested: they compute the project graph from inside a
// task of an outer `nx run`/`nx affected` (every `lint` target's boundary check,
// the affected-selection and boundary tests under `workspace:test`). With plugin
// isolation on, Nx starts a worker per default plugin and the worker exits if
// the host has not sent its load message within a fixed 10 seconds of
// connecting (nx/src/project-graph/plugins/isolation/plugin-worker.js), which
// a nested run on a loaded runner misses ("Failed to load 2 default Nx
// plugin(s)", seen on the Windows `check` leg). nx.json adds no plugins, so
// isolation buys nothing here: plugins load in-process, and should isolation
// ever be switched back on, its timeouts are lifted too.
import { execFileSync } from "node:child_process";
import { createRequire } from "node:module";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

export const NX_ENV = Object.freeze({
  // No resident daemon per checkout: nothing reaps it, and it buys little here.
  NX_DAEMON: "false",
  NX_NO_CLOUD: "true",
  NX_TUI: "false",
  NX_ISOLATE_PLUGINS: "false",
  NX_PLUGIN_NO_TIMEOUTS: "true",
});

/** The pinned Nx's bin entry for the checkout at `root`, as its own package.json declares it. */
export function nxBin(root) {
  const require = createRequire(join(root, "package.json"));
  const manifest = require.resolve("nx/package.json");
  return join(dirname(manifest), require(manifest).bin.nx);
}

/**
 * Run the pinned Nx under `NX_ENV` in `cwd` (resolved from `root`, the checkout
 * whose node_modules holds it), returning its stdout; throws as
 * `execFileSync` does, with both streams on the error.
 */
export function runNx(root, cwd, args) {
  return execFileSync(process.execPath, [nxBin(root), ...args], {
    cwd,
    env: { ...process.env, ...NX_ENV },
    encoding: "utf8",
    stdio: ["ignore", "pipe", "pipe"],
    maxBuffer: 64 * 1024 * 1024,
  });
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  for (const [key, value] of Object.entries(NX_ENV)) console.log(`${key}=${value}`);
}
