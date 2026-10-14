import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, utimesSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");

// Drive the published declaration's command and real pruner against isolated
// Cargo output; neither Cargo nor the pruning boundary is replaced by a double.
test("declared maintenance removes old superseded builds and keeps ordinary builds", {
  skip: process.platform !== "linux" && "pool hosts and the backdating journey are Linux",
}, () => {
  const cwd = mkdtempSync(join(process.env.ONEPIPELINE_NODE_SCRATCH_DIR || tmpdir(), "screencomp-maintenance-"));
  const run = (command, args, env = {}) => spawnSync(command, args, {
    cwd, encoding: "utf8", env: { ...process.env, ...env },
  });
  const succeeds = (result) => assert.equal(result.status, 0, result.stdout + result.stderr);
  try {
    writeFileSync(join(cwd, ".tool-versions"), readFileSync(join(root, ".tool-versions")));
    mkdirSync(join(cwd, "src"));
    writeFileSync(join(cwd, "Cargo.toml"), '[package]\nname = "maintenance-journey"\nversion = "0.1.0"\nedition = "2021"\n');
    writeFileSync(join(cwd, "src/main.rs"), 'fn main() { println!("maintenance journey"); }\n');
    const build = (metadata) => succeeds(run("cargo", ["build", "--offline", "--target-dir", "target"], {
      RUSTFLAGS: `-Cmetadata=${metadata}`,
    }));
    const entries = () => ["deps", ".fingerprint", "build", "incremental"].flatMap((kind) => {
      const path = join(cwd, "target/debug", kind);
      return existsSync(path) ? readdirSync(path).map((name) => join(path, name)) : [];
    });
    build("alternative");
    const alternative = entries();
    const backdate = (path) => {
      for (const entry of readdirSync(path, { withFileTypes: true })) {
        const child = join(path, entry.name);
        if (entry.isDirectory()) backdate(child);
        utimesSync(child, new Date(0), new Date(0));
      }
    };
    backdate(join(cwd, "target"));
    build("ordinary");
    const ordinary = entries().filter((path) => !alternative.includes(path));
    assert.ok(ordinary.length > 0, "ordinary build must create distinct metadata hashes");
    const declaration = readFileSync(join(root, "onevcs.toml"), "utf8");
    const command = JSON.parse(declaration.match(/^command = (.+)$/m)[1]);
    const maintain = () => run(command[0], ["--justfile", join(root, "justfile"), "--working-directory", cwd, ...command.slice(1)]);
    const result = maintain();
    succeeds(result);
    assert.match(result.stdout, /target/);
    assert.ok(alternative.some((path) => !existsSync(path)), "a superseded build must be removed");
    assert.ok(ordinary.every(existsSync), "every ordinary metadata entry must survive");
    succeeds(maintain());
    rmSync(join(cwd, "target"), { recursive: true });
    const missing = maintain();
    succeeds(missing);
    assert.match(missing.stderr + missing.stdout, /skipped/);
    assert.match(missing.stderr + missing.stdout, /target/);
    build("ordinary");
    const recovered = entries();
    succeeds(maintain());
    assert.ok(recovered.every(existsSync), "a fresh build after an empty slot must remain warm");
  } finally {
    rmSync(cwd, { recursive: true, force: true });
  }
});
