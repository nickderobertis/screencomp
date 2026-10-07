// Shared helpers for the gate tooling's tests: the repository root, Nx as the
// recipes run it, and a scratch copy of the working tree to mutate.
import { execFileSync } from "node:child_process";
import { cpSync, mkdirSync, mkdtempSync, rmSync, symlinkSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { runNx } from "../nx-env.mjs";

export const root = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");

/** Run the workspace's pinned Nx in `cwd` as every nested run does (nx-env.mjs), returning its stdout. */
export function nx(cwd, args) {
  return runNx(root, cwd, args);
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

/**
 * Windows refuses to remove a directory a live process still has as its cwd
 * (EBUSY), and a detached hand-off a test left running (the hung
 * setup-llmlint stand-in, for session-setup.test.mjs's HANG_SECONDS) holds the
 * scratch copy that way; retrying with backoff (up to ~21s in all) outlasts it.
 */
export function removeScratch(dir) {
  rmSync(dir, { recursive: true, force: true, maxRetries: 20, retryDelay: 100 });
}

/**
 * A `skip` reason for tests whose stand-ins are POSIX: `#!/bin/sh` scripts on a
 * `…:/usr/bin:/bin` PATH, which Windows cannot resolve. The Linux and macOS
 * `check` legs run them; false elsewhere.
 */
export const posixOnly =
  process.platform === "win32" && "the stand-ins here need a POSIX PATH; the Linux and macOS legs run this";
