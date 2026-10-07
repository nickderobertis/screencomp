// The module-boundary rule: the pure judgement over synthetic graphs, and the
// real check over the real Nx graph of a scratch copy whose project.json gains a
// forbidden edge.
import assert from "node:assert/strict";
import { execFileSync, spawnSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";

import { boundaryViolations, wellFormedAllow } from "../check-project-boundaries.mjs";
import { removeScratch, root, scratchCopy } from "./support.mjs";

const allow = JSON.parse(readFileSync(join(root, "nx.json"), "utf8")).boundaries.allow;
const nodes = {
  screencomp: { tags: ["type:app"] },
  "screencomp-e2e": { tags: ["type:e2e"] },
  "browser-tests": { tags: ["type:browser-test"] },
  "visual-docs-actions": { tags: ["type:actions"] },
  workspace: { tags: ["type:workspace"] },
};
const judge = (edges, cargoEdges = []) =>
  boundaryViolations({ nodes, edges, cargoEdges, allow, checked: new Set(Object.keys(nodes)) });

test("nx.json's boundaries table is well formed", () => {
  assert.ok(wellFormedAllow(allow));
  assert.ok(!wellFormedAllow({ "type:a": ["type:missing"] }));
});

test("the repository's edges are allowed", () => {
  assert.deepEqual(
    judge([
      ["screencomp-e2e", "screencomp"],
      ["browser-tests", "screencomp"],
      ["visual-docs-actions", "screencomp"],
      ["workspace", "screencomp"],
      ["workspace", "screencomp-e2e"],
    ]),
    [],
  );
});

test("nothing but the workspace aggregate may depend on a test suite", () => {
  for (const [source, target] of [
    ["screencomp", "screencomp-e2e"],
    ["visual-docs-actions", "screencomp-e2e"],
    ["visual-docs-actions", "browser-tests"],
    ["workspace", "browser-tests"],
    ["screencomp-e2e", "browser-tests"],
  ]) {
    const problems = judge([[source, target]]);
    assert.equal(problems.length, 1, `${source} -> ${target}`);
    assert.match(problems[0], new RegExp(`^${source} \\(type:[a-z0-9-]+\\) -> ${target} \\(type:[a-z0-9-]+\\) is not allowed`));
  }
});

test("a Cargo path dependency must also be an Nx edge", () => {
  const problems = judge([], [["screencomp-e2e", "screencomp"]]);
  assert.equal(problems.length, 1);
  assert.match(problems[0], /Cargo edge screencomp-e2e -> screencomp is missing from the Nx graph/);
});

test("every project carries exactly one declared type tag", () => {
  const problems = boundaryViolations({
    nodes: { a: { tags: [] }, b: { tags: ["type:app", "type:e2e"] }, c: { tags: ["type:nope"] } },
    edges: [],
    allow,
    checked: new Set(["a", "b", "c"]),
  });
  assert.equal(problems.length, 3);
});

test("the real check fails naming an edge drawn to the e2e suite, and passes without it", () => {
  const dir = scratchCopy();
  try {
    const check = (project) =>
      spawnSync(process.execPath, ["tools/workspace/check-project-boundaries.mjs", project], {
        cwd: dir,
        encoding: "utf8",
        env: { ...process.env, NX_DAEMON: "false", NX_NO_CLOUD: "true" },
      });
    const clean = check("visual-docs-actions");
    assert.equal(clean.status, 0, clean.stderr);
    assert.match(clean.stdout, /visual-docs-actions: every edge allowed/);

    const path = join(dir, "tools/visual-docs-actions/project.json");
    const project = JSON.parse(readFileSync(path, "utf8"));
    project.implicitDependencies.push("screencomp-e2e");
    writeFileSync(path, JSON.stringify(project, null, 2));
    const drawn = check("visual-docs-actions");
    assert.equal(drawn.status, 1);
    assert.match(
      drawn.stderr,
      /visual-docs-actions \(type:actions\) -> screencomp-e2e \(type:e2e\) is not allowed/,
    );
  } finally {
    removeScratch(dir);
  }
});

test("the check passes on this checkout", () => {
  const out = execFileSync(process.execPath, ["tools/workspace/check-project-boundaries.mjs"], {
    cwd: root,
    encoding: "utf8",
    env: { ...process.env, NX_DAEMON: "false", NX_NO_CLOUD: "true" },
  });
  assert.match(out, /every edge allowed/);
});
