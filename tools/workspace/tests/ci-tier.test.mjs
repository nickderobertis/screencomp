// The CI tier selector: which tier each GitHub event runs, through the pure
// selection and through the script exactly as ci.yml runs it (a real event file,
// real git, the real release-plz.yml).
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import { releaseBranchPrefix, selectTier } from "../ci-tier.mjs";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const script = join(root, "tools/workspace/ci-tier.mjs");
const BASE = "a".repeat(40);
const MERGE_BASE = "b".repeat(40);
const mergeBase = (sha) => (sha === BASE ? MERGE_BASE : null);

test("release-plz.yml names the release branch prefix the selector sweeps", () => {
  const workflow = readFileSync(join(root, ".github/workflows/release-plz.yml"), "utf8");
  assert.equal(releaseBranchPrefix(workflow), "release-plz-");
  assert.equal(releaseBranchPrefix("no merge step here"), null);
  assert.equal(releaseBranchPrefix('startswith("bad prefix; rm -rf")'), null);
});

test("release-plz's release pull request runs the full sweep", () => {
  const event = { pull_request: { head: { ref: "release-plz-2026-10-07T12-00-00Z" }, base: { sha: BASE } } };
  const got = selectTier({ eventName: "pull_request", event, releasePrefix: "release-plz-", mergeBase });
  assert.equal(got.tier, "all");
  assert.equal(got.base, "");
});

test("an ordinary pull request runs the affected tier against the merge base", () => {
  const event = { pull_request: { head: { ref: "feat/thing" }, base: { sha: BASE } } };
  const got = selectTier({ eventName: "pull_request", event, releasePrefix: "release-plz-", mergeBase });
  assert.deepEqual([got.tier, got.base], ["affected", MERGE_BASE]);
});

test("a push to main runs the affected tier against the replaced tip's merge base", () => {
  const event = { ref: "refs/heads/main", before: BASE };
  const got = selectTier({ eventName: "push", event, releasePrefix: "release-plz-", mergeBase });
  assert.deepEqual([got.tier, got.base], ["affected", MERGE_BASE]);
});

test("an underivable base fails closed into the sweep", () => {
  const cases = [
    ["pull_request", { pull_request: { head: { ref: "feat/x" }, base: { sha: "c".repeat(40) } } }],
    ["pull_request", { pull_request: { head: { ref: "feat/x" }, base: { sha: "$(touch pwned)" } } }],
    ["push", { ref: "refs/heads/main", before: "0".repeat(40) }],
    ["pull_request", null],
    ["workflow_dispatch", {}],
  ];
  for (const [eventName, event] of cases) {
    const got = selectTier({ eventName, event, releasePrefix: "release-plz-", mergeBase });
    assert.equal(got.tier, "all", `${eventName} ${JSON.stringify(event)}`);
    assert.equal(got.base, "");
  }
});

test("no readable release prefix sweeps every pull request", () => {
  const event = { pull_request: { head: { ref: "feat/x" }, base: { sha: BASE } } };
  assert.equal(selectTier({ eventName: "pull_request", event, releasePrefix: null, mergeBase }).tier, "all");
});

/** Run the script as ci.yml does: an event file, the event name, this checkout. */
function runScript(eventName, event) {
  const dir = mkdtempSync(join(tmpdir(), "screencomp-ci-tier-"));
  try {
    const path = join(dir, "event.json");
    writeFileSync(path, JSON.stringify(event));
    const out = execFileSync(process.execPath, [script], {
      cwd: root,
      env: { ...process.env, GITHUB_EVENT_NAME: eventName, GITHUB_EVENT_PATH: path },
      encoding: "utf8",
      stdio: ["ignore", "pipe", "pipe"],
    });
    return Object.fromEntries(out.trim().split("\n").map((line) => line.split("=", 2)));
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

test("the script prints the GITHUB_OUTPUT lines the check job reads", () => {
  const head = execFileSync("git", ["rev-parse", "HEAD"], { cwd: root, encoding: "utf8" }).trim();
  const release = runScript("pull_request", {
    pull_request: { head: { ref: "release-plz-2026-10-07T12-00-00Z" }, base: { sha: head } },
  });
  assert.deepEqual(release, { tier: "all", base: "" });
  const ordinary = runScript("pull_request", { pull_request: { head: { ref: "fix/x" }, base: { sha: head } } });
  assert.deepEqual(ordinary, { tier: "affected", base: head });
});
