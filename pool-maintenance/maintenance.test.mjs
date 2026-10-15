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

// Drive measure.sh, the budget's command, end to end against a one-crate Cargo
// fixture: the real declared command and pruner run; only the gate's test build
// is swapped for the fixture's own cargo build so no workspace build happens.
const measure = (prepare, gateBuild, env = {}) => {
  const cwd = mkdtempSync(join(process.env.ONEPIPELINE_NODE_SCRATCH_DIR || tmpdir(), "screencomp-measure-"));
  try {
    writeFileSync(join(cwd, ".tool-versions"), readFileSync(join(root, ".tool-versions")));
    mkdirSync(join(cwd, "src"));
    writeFileSync(join(cwd, "Cargo.toml"), '[package]\nname = "measure-journey"\nversion = "0.1.0"\nedition = "2021"\n');
    writeFileSync(join(cwd, "src/main.rs"), "fn main() {}\n");
    prepare(cwd);
    const report = join(cwd, "report.json");
    const result = spawnSync(join(root, "pool-maintenance/measure.sh"), [], {
      cwd: join(root, "pool-maintenance"),
      encoding: "utf8",
      env: {
        ...process.env,
        MEASURE_WORKSPACE: cwd,
        MEASURE_EVIDENCE: join(cwd, "evidence"),
        MEASURE_GATE_BUILD: gateBuild,
        ONEBUDGETSPEC_RESULT: report,
        ...(typeof env === "function" ? env(cwd) : env),
      },
    });
    return {
      ...result,
      report: existsSync(report) ? readFileSync(report, "utf8") : "",
      kept: existsSync(join(cwd, "foreign/keep.txt")),
    };
  } finally {
    rmSync(cwd, { recursive: true, force: true });
  }
};
const linuxOnly = { skip: process.platform !== "linux" && "the measurement relies on GNU find and touch" };

test("measure.sh reports only the timed pass, which removes the backdated alternative build", linuxOnly, () => {
  const result = measure(() => {}, "fixture-dev");
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stderr, /measure\.sh: removed [1-9]\d* of the alternative build's \d+ entries; all [1-9]\d* ordinary entries/);
  const report = JSON.parse(result.report);
  // Only the report's shape: the budget's threshold is onebudgetspec's to enforce.
  assert.ok(Number.isFinite(report.value) && report.value >= 0, `figure ${report.value}`);
  assert.match(report.detail, /builds removed: [1-9]/);
  assert.match(report.detail, /bytes freed: [1-9]/);
  assert.match(report.detail, /untimed backlog pass beforehand: [\d.]+ s \(not in the figure\)$/);
});

test("measure.sh fails naming the step when the declared maintenance fails", linuxOnly, () => {
  // A regular file where the target dir belongs is the pruner's own refusal.
  const result = measure((cwd) => writeFileSync(join(cwd, "target"), ""), "fixture-dev");
  assert.equal(result.status, 1, result.stderr);
  assert.match(result.stderr, /Not a directory/);
  assert.match(result.stderr, /measure\.sh: the untimed backlog pass failed/);
  assert.match(result.stderr, /ACTION: fix the error 'just pool-maintenance' reports above, then rerun \.\/measure\.sh/);
  assert.equal(result.report, "");
});

test("measure.sh fails rather than report a figure when the timed pass removes nothing", linuxOnly, () => {
  // A release build ignores the alternative dev-profile setting, so the
  // backdated build stays each unit's newest and nothing is superseded.
  const result = measure(() => {}, "fixture-release");
  assert.equal(result.status, 1, result.stderr);
  assert.match(result.stderr, /measure\.sh: the timed pass removed none of the alternative build's entries/);
  assert.doesNotMatch(result.stderr, /measure\.sh: removed/);
  assert.doesNotMatch(result.report, /untimed backlog pass/);
});

test("measure.sh refuses inputs outside its boundary before touching anything", linuxOnly, () => {
  const foreign = (cwd) => {
    mkdirSync(join(cwd, "foreign"));
    writeFileSync(join(cwd, "foreign/keep.txt"), "not measure.sh's");
  };
  const refusals = [
    [{ MEASURE_WORKSPACE: "relative" }, /MEASURE_WORKSPACE 'relative' is not an absolute path/],
    [{ MEASURE_WORKSPACE: tmpdir() }, /MEASURE_WORKSPACE '.*' holds no Cargo\.toml/],
    [{ MEASURE_GATE_BUILD: "rm -rf /" }, /MEASURE_GATE_BUILD 'rm -rf \/' is not one of gate, fixture-dev, fixture-release/],
    [{ ONEBUDGETSPEC_RESULT: "report.json" }, /ONEBUDGETSPEC_RESULT 'report\.json' is not an absolute path/],
    [(cwd) => ({ ONEBUDGETSPEC_RESULT: join(cwd, "missing/report.json") }), /is not a file path in an existing directory/],
    [{ MEASURE_EVIDENCE: "evidence" }, /MEASURE_EVIDENCE 'evidence' is not an absolute path/],
    [(cwd) => ({ MEASURE_EVIDENCE: join(cwd, "foreign") }), /exists and is not an evidence directory this script made/],
  ];
  for (const [env, message] of refusals) {
    const result = measure(foreign, "fixture-dev", env);
    assert.equal(result.status, 1, result.stderr);
    assert.match(result.stderr, message);
    assert.match(result.stderr, /^ACTION: /m);
    assert.ok(result.kept, "a directory measure.sh did not make must survive");
    assert.equal(result.report, "");
  }
});
