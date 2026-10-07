// Which projects a change selects, as Nx's own affected detection computes it
// over this repository's graph. The cases are the ones the graph exists for: a
// change to an action reaches only the actions' project, never the crate's
// suites; a change to the crate reaches every project built on it.
import assert from "node:assert/strict";
import { test } from "node:test";

import { affectedBy } from "./support.mjs";

const CRATE_AND_DEPENDENTS = [
  "browser-tests",
  "docker-image",
  "screencomp",
  "screencomp-e2e",
  "visual-docs-actions",
  "workspace",
];

test("a change under src/ selects the crate, its e2e suite and every dependent", () => {
  assert.deepEqual(affectedBy(["src/lib.rs"]), CRATE_AND_DEPENDENTS);
  assert.deepEqual(affectedBy(["tests/integration.rs"]), CRATE_AND_DEPENDENTS);
  assert.deepEqual(affectedBy(["Cargo.lock"]), CRATE_AND_DEPENDENTS);
});

test("a change confined to the composite actions or their scripts selects only their project", () => {
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
    assert.deepEqual(affectedBy([file]), ["visual-docs-actions"], file);
  }
});

test("each suite and surface selects its own project and its dependents only", () => {
  assert.deepEqual(affectedBy(["e2e/tests/e2e.rs"]), ["screencomp-e2e", "workspace"]);
  assert.deepEqual(affectedBy(["browser-tests/tests/gallery.spec.ts"]), ["browser-tests"]);
  assert.deepEqual(affectedBy(["Dockerfile"]), ["docker-image"]);
  assert.deepEqual(affectedBy(["demo/screencomp.toml"]), ["demo", "screencomp-e2e", "visual-docs-actions", "workspace"]);
  assert.deepEqual(affectedBy(["tools/workspace/ci-tier.mjs"]), ["workspace"]);
});

test("a documentation-only change outside every project selects nothing", () => {
  assert.deepEqual(affectedBy(["CONTRIBUTING.md"]), []);
});
