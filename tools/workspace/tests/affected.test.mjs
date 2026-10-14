// Which projects a change selects, as Nx's own affected detection computes it
// over this repository's graph. The cases are the ones the graph exists for: a
// change to an action reaches only the actions' project, never the crate's
// suites; a change to the crate's sources reaches every project built on it; a
// change to a suite or the benchmarks reaches only that suite (and the coverage
// aggregate over it).
import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";

import { affectedBy, nx, root } from "./support.mjs";

const CRATE_AND_DEPENDENTS = [
  "browser-tests",
  "coverage",
  "crate-checks",
  "docker-image",
  "release-packaging",
  "screencomp",
  "screencomp-bench",
  "screencomp-cli",
  "screencomp-e2e",
  "visual-docs-actions",
];

test("a change under src/ selects the crate, its e2e suite and every dependent", () => {
  assert.deepEqual(affectedBy(["src/lib.rs"]), CRATE_AND_DEPENDENTS);
  assert.deepEqual(affectedBy(["Cargo.lock"]), CRATE_AND_DEPENDENTS);
});

test("a change confined to the crate's suites or benchmarks selects only them", () => {
  assert.deepEqual(affectedBy(["tests/integration.rs"]), ["coverage", "screencomp"]);
  assert.deepEqual(affectedBy(["tests/actions.rs"]), ["coverage", "screencomp", "visual-docs-actions"]);
  assert.deepEqual(affectedBy(["benches/commands.rs"]), ["screencomp-bench"]);
});

test("action changes select their owner, with the reusable workflow also selecting its graph proof", () => {
  for (const file of [
    "action.yml",
    "visual-docs/action.yml",
    "visual-docs-aggregate/action.yml",
    "visual-docs-pages/action.yml",
    "gh-pages-maintenance/action.yml",
    ".github/workflows/visual-docs-reusable.yml",
    "scripts/visual-docs-pages-build.sh",
    "scripts/visual-docs-gh-pages.sh",
  ]) {
    assert.deepEqual(affectedBy([file]), (file === "action.yml" ? ["release-packaging", "visual-docs-actions"] : file === ".github/workflows/visual-docs-reusable.yml" ? ["github-workflows", "visual-docs-actions"] : ["visual-docs-actions"]), file);
  }
});

test("each suite and surface selects its own project and its dependents only", () => {
  // The gate's own tooling never selects the coverage aggregate or a test suite.
  assert.deepEqual(affectedBy(["e2e/tests/e2e.rs"]), ["coverage", "screencomp-e2e"]);
  assert.deepEqual(affectedBy(["browser-tests/tests/gallery.spec.ts"]), ["browser-tests"]);
  assert.deepEqual(affectedBy(["Dockerfile"]), ["docker-image"]);
  assert.deepEqual(affectedBy(["demo/screencomp.toml"]), ["coverage", "demo", "screencomp-e2e"]);
  assert.deepEqual(affectedBy(["demo/capture.sh"]), ["demo", "visual-docs-actions"]);
  assert.deepEqual(affectedBy(["demo/pages/index.html"]), ["demo"]);
  assert.deepEqual(affectedBy(["tools/workspace/ci-tier.mjs"]), ["github-workflows", "workspace"]);
  assert.deepEqual(affectedBy(["scripts/hooks-install.sh"]), ["workspace"]);
  assert.deepEqual(affectedBy(["lefthook.yml"]), ["workspace"]);
});

test("a documentation-only change outside every project selects nothing", () => {
  assert.deepEqual(affectedBy(["CONTRIBUTING.md"]), []);
});

test("workflow tooling selects its telemetry and budgets without a crate dependency", () => {
  for (const path of [".github/macos-policy.mjs", ".github/budgets.yaml"]) assert.deepEqual(affectedBy([path]), ["github-workflows"]);
});

test("release packaging contract participants select the owning production journey", () => {
  for (const file of ["action.yml", ".github/workflows/release.yml", "scripts/tests/release-artifact.test.mjs"]) assert.ok(affectedBy([file]).includes("release-packaging"), file);
});

test("the pool-maintenance project's own files select only it", () => {
  for (const file of ["pool-maintenance/measure.sh", "pool-maintenance/budgets.yaml"]) {
    assert.deepEqual(affectedBy([file]), ["pool-maintenance"], file);
  }
  // Every project.json is an input of the graph's own tests.
  assert.deepEqual(affectedBy(["pool-maintenance/project.json"]), ["pool-maintenance", "workspace"]);
});

/** The targets `just check` runs, read from the recipe's run-many line. */
function gateTargets() {
  const line = readFileSync(join(root, "justfile"), "utf8").match(/nx run-many -t ([^\n]+)/);
  assert.ok(line, "the check recipe's run-many line");
  return line[1].trim().split(/\s+/);
}

/** The task ids Nx would run for `args` (a run-many or affected selection) over `targets`. */
function taskIds(args, targets = gateTargets()) {
  const dir = mkdtempSync(join(tmpdir(), "screencomp-graph-"));
  try {
    const path = join(dir, "graph.json");
    nx(root, [...args, "-t", ...targets, `--graph=${path}`]);
    return Object.keys(JSON.parse(readFileSync(path, "utf8")).tasks.tasks);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

test("no gate selection runs the pool-maintenance measurement, only its script lint", () => {
  const selections = [
    ["run-many"],
    ...[
      ["src/lib.rs"],
      ["pool-maintenance/measure.sh"],
      ["pool-maintenance/budgets.yaml"],
      ["pool-maintenance/project.json", "nx.json"],
      ["justfile", "onevcs.toml"],
    ].map((files) => ["affected", `--files=${files.join(",")}`]),
  ];
  for (const args of selections) {
    const pool = taskIds(args).filter((id) => id.startsWith("pool-maintenance:"));
    assert.ok(pool.every((id) => id === "pool-maintenance:lint"), `${args.join(" ")}: ${pool.join(", ")}`);
  }
  // The measurement is a real target, reached only by naming it (`just budgets-host`).
  assert.deepEqual(taskIds(["run-many"], ["budgets-host"]), ["pool-maintenance:budgets-host"]);
  assert.ok(taskIds(["run-many"]).includes("pool-maintenance:lint"), "the sweep lints the measurement script");
});
