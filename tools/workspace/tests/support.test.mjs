// The scratch-copy helpers the other gate-tooling tests share.
import assert from "node:assert/strict";
import { existsSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";

import { removeScratch } from "./support.mjs";

function busy() {
  return Object.assign(new Error("EBUSY: resource busy or locked, rmdir"), { code: "EBUSY" });
}

test("removeScratch keeps retrying a directory Windows still holds busy, then removes it", () => {
  const dir = mkdtempSync(join(tmpdir(), "screencomp-remove-"));
  writeFileSync(join(dir, "file"), "x");
  // Windows's EBUSY on the final rmdir, for as long as a hung process holds it.
  let refusals = 3;
  removeScratch(dir, (path, options) => {
    if (refusals-- > 0) throw busy();
    rmSync(path, options);
  });
  assert.equal(refusals, -1);
  assert.ok(!existsSync(dir));
});

test("removeScratch rethrows an error that waiting cannot clear", () => {
  const dir = mkdtempSync(join(tmpdir(), "screencomp-remove-"));
  try {
    let calls = 0;
    const denied = Object.assign(new Error("EACCES"), { code: "EACCES" });
    assert.throws(
      () =>
        removeScratch(dir, () => {
          calls += 1;
          throw denied;
        }),
      denied,
    );
    assert.equal(calls, 1);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
