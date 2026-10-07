// Shared helpers for the gate tooling's tests: the repository root, Nx as the
// recipes run it, and a scratch copy of the working tree to mutate.
import { execFileSync } from "node:child_process";
import { cpSync, mkdirSync, mkdtempSync, rmSync, symlinkSync } from "node:fs";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

export const root = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");

const require = createRequire(join(root, "package.json"));
const manifest = require.resolve("nx/package.json");
const nxBin = join(dirname(manifest), require(manifest).bin.nx);

/** Run the workspace's pinned Nx in `cwd`, returning its stdout. */
export function nx(cwd, args) {
  return execFileSync(process.execPath, [nxBin, ...args], {
    cwd,
    env: { ...process.env, NX_DAEMON: "false", NX_NO_CLOUD: "true", NX_TUI: "false" },
    encoding: "utf8",
    stdio: ["ignore", "pipe", "pipe"],
    maxBuffer: 64 * 1024 * 1024,
  });
}

/** The projects a change to `files` selects, as Nx's affected detection computes them. */
export function affectedBy(files, cwd = root) {
  return JSON.parse(nx(cwd, ["show", "projects", "--affected", `--files=${files.join(",")}`, "--json"])).sort();
}

/**
 * A scratch copy of the working tree (tracked and untracked, ignored files left
 * out) sharing this checkout's node_modules, for tests that edit the graph.
 * Returns its path; remove it with `rmSync(path, { recursive: true })`.
 */
export function scratchCopy() {
  const dir = mkdtempSync(join(tmpdir(), "screencomp-scratch-"));
  const files = execFileSync("git", ["ls-files", "-z", "--cached", "--others", "--exclude-standard"], {
    cwd: root,
    encoding: "utf8",
  })
    .split("\0")
    .filter(Boolean);
  for (const file of files) {
    mkdirSync(dirname(join(dir, file)), { recursive: true });
    try {
      cpSync(join(root, file), join(dir, file));
    } catch (error) {
      // A tracked file deleted in the working tree is simply not copied.
      if (error.code !== "ENOENT") throw error;
    }
  }
  symlinkSync(join(root, "node_modules"), join(dir, "node_modules"), "junction");
  return dir;
}

export function removeScratch(dir) {
  rmSync(dir, { recursive: true, force: true });
}
