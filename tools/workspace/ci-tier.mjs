#!/usr/bin/env node
// Which gate tier a CI run is for, read from the GitHub event that started it.
//
//   node tools/workspace/ci-tier.mjs >> "$GITHUB_OUTPUT"
//
// Prints `tier=…` and `base=…` lines; the workflow passes `tier` to the gate
// recipe (`just check "$TIER"`) and `base` as NX_BASE, so the tier is a flag on
// the one recipe and never a second implementation of the gate.
//
// Releases are batched behind release-plz's release pull request, so that pull
// request runs the sweep and everything else the affected tier (AGENTS.md,
// "Release & git"); `selectTier` below is the whole policy.
//
// A base that cannot be derived (a first push, a force-push whose old tip is not
// in the checkout, a shallow clone) fails closed into the sweep and says so,
// rather than scoping a run against nothing. Every value printed is a git object
// name git itself resolved or a fixed word: nothing from the event payload
// reaches the output unvalidated.
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const SHA = /^[0-9a-f]{40}([0-9a-f]{24})?$/;
const PREFIX = /^[A-Za-z0-9._/-]+$/;

/**
 * The branch prefix release-plz opens its release pull request from, read from
 * the merge step of `release-plz.yml`, the one place this repository names it,
 * so the pull request swept here is the one that workflow merges. Null when it
 * cannot be read, which sweeps every pull request.
 */
export function releaseBranchPrefix(workflowText) {
  const prefix = workflowText.match(/startswith\("([^"]+)"\)/)?.[1] ?? null;
  return prefix && PREFIX.test(prefix) ? prefix : null;
}

/**
 * The tier for one event, as `{ tier, base, why }`. Pure: `mergeBase(sha)`
 * returns the merge base of `sha` and the checkout's HEAD, or null.
 */
export function selectTier({ eventName, event, releasePrefix, mergeBase }) {
  const sweep = (why) => ({ tier: "all", base: "", why });
  if (event === null || typeof event !== "object" || Array.isArray(event)) {
    return sweep(`no readable event object for '${eventName}', so nothing scopes the run`);
  }
  const baseOf = (sha) => (typeof sha === "string" && SHA.test(sha) && !/^0+$/.test(sha) ? mergeBase(sha) : null);

  if (eventName === "pull_request") {
    if (!releasePrefix) {
      return sweep("release-plz.yml names no release branch prefix, so no pull request can be scoped");
    }
    const head = event.pull_request?.head?.ref;
    if (typeof head === "string" && head.startsWith(releasePrefix)) {
      return sweep("release-plz's release pull request: the commit that ships is swept here");
    }
    const base = baseOf(event.pull_request?.base?.sha);
    if (!base) return sweep("the pull request's base commit is not in this checkout (fetch-depth: 0)");
    return { tier: "affected", base, why: `pull request, merge base ${base} with its base branch` };
  }

  if (eventName === "push" && event.ref === "refs/heads/main") {
    const base = baseOf(event.before);
    if (!base) return sweep("the commit this push replaced is not in this checkout");
    return { tier: "affected", base, why: `push to main, merge base ${base} with the replaced tip` };
  }

  return sweep(`'${eventName}' is neither a pull request nor a push to main`);
}

function gitMergeBase(sha) {
  try {
    const base = execFileSync("git", ["merge-base", sha, "HEAD"], {
      encoding: "utf8",
      stdio: ["ignore", "pipe", "ignore"],
    }).trim();
    return SHA.test(base) ? base : null;
  } catch {
    return null;
  }
}

function main() {
  const root = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
  let event = null;
  try {
    event = JSON.parse(readFileSync(process.env.GITHUB_EVENT_PATH ?? "", "utf8"));
  } catch {
    // Handled by selectTier with every other payload that is not an object.
  }
  let releasePrefix = null;
  try {
    releasePrefix = releaseBranchPrefix(readFileSync(join(root, ".github/workflows/release-plz.yml"), "utf8"));
  } catch {
    // Null sweeps every pull request.
  }
  const { tier, base, why } = selectTier({
    eventName: process.env.GITHUB_EVENT_NAME ?? "",
    event,
    releasePrefix,
    mergeBase: gitMergeBase,
  });
  console.error(`ci-tier: ${tier} tier: ${why}`);
  console.log(`tier=${tier}`);
  console.log(`base=${base}`);
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main();
