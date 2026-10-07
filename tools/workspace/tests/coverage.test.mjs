// The combined coverage gate's wiring, read from the resolved Nx graph and the
// justfile recipes the targets call. Running the instrumented suites themselves
// takes minutes and gigabytes, too much for this tier on every tooling change;
// their end-to-end proof (both suites' profiles merged, the floor passing on the
// tree and failing once an untested function lands under src/) is a scratch-copy
// journey run through `just check`. This pins what that proof depends on, so the
// aggregate cannot silently lose a suite, merge stale profiles, or drop the floor.
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { test } from "node:test";

import { nx, root } from "./support.mjs";

const project = (name) => JSON.parse(nx(root, ["show", "project", name, "--json"]));
const recipe = (name) => execFileSync("just", ["--show", name], { cwd: root, encoding: "utf8" });
const PROFILES = "{workspaceRoot}/target/llvm-cov-target";

test("both Rust suites write deferred profiles after the clean, into the declared outputs", () => {
  for (const [name, args] of [
    ["screencomp", "screencomp"],
    ["screencomp-e2e", "screencomp-e2e screencomp"],
  ]) {
    const target = project(name).targets.test;
    assert.equal(target.options.command, `just _rust-test ${args}`, name);
    assert.deepEqual(target.outputs, [PROFILES], name);
    assert.ok(
      target.dependsOn.some((dep) => dep.target === "coverage-clean" && dep.projects.includes("coverage")),
      `${name}: test must follow coverage:coverage-clean`,
    );
  }
  assert.match(recipe("_rust-test"), /cargo llvm-cov --no-report nextest --locked --all-features -p \{\{ ?crate ?\}\}/);
  assert.match(recipe("_coverage-clean"), /cargo llvm-cov clean --workspace/);
});

test("the aggregate merges both suites and enforces the 95% floor", () => {
  const coverage = project("coverage").targets.coverage;
  assert.equal(coverage.options.command, "just _coverage");
  assert.deepEqual(coverage.outputs, [PROFILES]);
  const suites = coverage.dependsOn.find((dep) => dep.target === "test");
  assert.deepEqual([...suites.projects].sort(), ["screencomp", "screencomp-e2e"]);
  assert.match(recipe("_coverage"), /cargo llvm-cov report --fail-under-lines \{\{ ?cov_min ?\}\}/);
  assert.equal(execFileSync("just", ["--evaluate", "cov_min"], { cwd: root, encoding: "utf8" }).trim(), "95");
});

test("the gate runs the aggregate", () => {
  assert.match(recipe("check"), /nx affected --base="\$base" -t [^\n]*\bcoverage\b/);
  assert.match(recipe("check"), /nx run-many -t [^\n]*\bcoverage\b/);
});
