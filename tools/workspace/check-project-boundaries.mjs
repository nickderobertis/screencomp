#!/usr/bin/env node
// The module-boundary rule, over the real project graph.
//
//   node tools/workspace/check-project-boundaries.mjs [PROJECT ...]
//
// Checks the outgoing edges of each named project (every project when none is
// named). Every project's `lint` target runs it for itself, so a change that
// draws a new edge fails the lint of the project that drew it.
//
// The edges are the union of two sources, so an edge cannot hide in either:
//   * the graph Nx itself computes (`nx graph --file`): every
//     `implicitDependencies` entry and anything Nx infers;
//   * Cargo's path dependencies between workspace members (`cargo metadata`),
//     each crate being the Nx project of the same name. Each must also be in
//     the Nx graph, or affected detection would not know that a change to the
//     dependency reaches the dependent.
// Every edge is then held to nx.json's `boundaries.allow`: a project tagged
// `type:X` may depend only on projects whose `type:` tag `allow["type:X"]` lists.
// Each project carries exactly one `type:` tag.
//
// Quiet on success (one line); on failure, one line per violation naming the
// edge, then the fix.
import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const TYPE_TAG = /^type:[a-z][a-z0-9-]*$/;

/**
 * The violations of `allow` in a project graph, as messages (empty when every
 * edge is allowed). Pure: `nodes` maps a project name to its `{ tags }`,
 * `edges` is a list of `[source, target]` project names, and `cargoEdges` lists
 * `[fromCrate, toCrate]` path dependencies between workspace crates.
 * Only edges leaving a project in `checked` are judged.
 */
export function boundaryViolations({ nodes, edges, cargoEdges = [], allow, checked }) {
  const problems = [];
  const typeOf = {};
  for (const [name, node] of Object.entries(nodes)) {
    const types = (node.tags ?? []).filter((tag) => tag.startsWith("type:"));
    if (types.length !== 1) {
      problems.push(`${name} carries ${types.length} type: tags (${types.join(", ") || "none"}); give it exactly one`);
    } else if (!Object.hasOwn(allow, types[0])) {
      problems.push(`${name} is tagged ${types[0]}, which nx.json "boundaries.allow" does not declare`);
    } else {
      typeOf[name] = types[0];
    }
  }

  const all = new Map();
  for (const [source, target] of edges) {
    if (Object.hasOwn(nodes, target)) all.set(`${source} -> ${target}`, [source, target]);
  }
  for (const [source, target] of cargoEdges) {
    if (!Object.hasOwn(nodes, source) || !Object.hasOwn(nodes, target)) {
      problems.push(`Cargo edge ${source} -> ${target} joins a crate with no Nx project of the same name`);
      continue;
    }
    if (!all.has(`${source} -> ${target}`) && checked.has(source)) {
      problems.push(
        `Cargo edge ${source} -> ${target} is missing from the Nx graph, so a change to ${target} would not select ${source}; add "${target}" to ${source}'s implicitDependencies`,
      );
    }
    all.set(`${source} -> ${target}`, [source, target]);
  }

  for (const [source, target] of all.values()) {
    if (!checked.has(source) || !typeOf[source] || !typeOf[target]) continue;
    const allowed = allow[typeOf[source]];
    if (!allowed.includes(typeOf[target])) {
      problems.push(
        `${source} (${typeOf[source]}) -> ${target} (${typeOf[target]}) is not allowed: ${typeOf[source]} may depend only on [${allowed.join(", ")}] (nx.json "boundaries.allow")`,
      );
    }
  }
  return problems;
}

/** Whether `allow` is a well-formed `boundaries.allow` table. */
export function wellFormedAllow(allow) {
  return (
    allow !== null &&
    typeof allow === "object" &&
    !Array.isArray(allow) &&
    Object.entries(allow).every(
      ([type, allowed]) =>
        TYPE_TAG.test(type) &&
        Array.isArray(allowed) &&
        allowed.every((target) => typeof target === "string" && Object.hasOwn(allow, target)),
    )
  );
}

function fail(lines) {
  for (const line of lines) console.error(`check-project-boundaries: ${line}`);
  process.exit(1);
}

/** `what`'s failure, with its own output and the next step, as a failed run. */
function failedRun(what, error, action) {
  // Nx reports some failures on stdout, so both streams are kept.
  const output = [error.stdout, error.stderr ?? error.message ?? error]
    .map((stream) => String(stream ?? "").trim())
    .filter(Boolean)
    .join("\n");
  fail([`${what} failed: ${output || "no output"}`, `ACTION: ${action}`]);
}

/** The project graph exactly as Nx computes it for the checkout at `root`. */
function nxGraph(root) {
  const require = createRequire(join(root, "package.json"));
  const scratch = mkdtempSync(join(tmpdir(), "screencomp-graph-"));
  try {
    const manifest = require.resolve("nx/package.json");
    const nx = join(dirname(manifest), require(manifest).bin.nx);
    const file = join(scratch, "graph.json");
    // Plugins load in-process: nx.json adds none, so isolation only buys a
    // worker handshake with a fixed 10s deadline, which this nested run misses
    // on a loaded runner (seen on the Windows leg) while the outer `nx run`
    // keeps every core busy.
    execFileSync(process.execPath, [nx, "graph", `--file=${file}`], {
      cwd: root,
      env: { ...process.env, NX_DAEMON: "false", NX_NO_CLOUD: "true", NX_ISOLATE_PLUGINS: "false" },
      stdio: ["ignore", "pipe", "pipe"],
      maxBuffer: 64 * 1024 * 1024,
    });
    const graph = JSON.parse(readFileSync(file, "utf8")).graph;
    const shaped =
      graph?.nodes &&
      typeof graph.nodes === "object" &&
      graph.dependencies &&
      typeof graph.dependencies === "object" &&
      Object.values(graph.nodes).every(
        (node) =>
          typeof node?.data?.root === "string" &&
          (node.data.tags === undefined ||
            (Array.isArray(node.data.tags) && node.data.tags.every((tag) => typeof tag === "string"))),
      ) &&
      Object.values(graph.dependencies).every(
        (deps) => Array.isArray(deps) && deps.every((dep) => typeof dep?.target === "string"),
      );
    if (!shaped) throw new Error("its `nodes` / `dependencies` are not the shape this check reads");
    return graph;
  } catch (error) {
    return failedRun(
      "computing the Nx project graph (`nx graph`)",
      error,
      "run `bash tools/workspace/node-modules.sh` to heal the Nx install, then fix the project.json the message names",
    );
  } finally {
    rmSync(scratch, { recursive: true, force: true });
  }
}

/** Cargo's path dependencies between workspace members, as [fromCrate, toCrate]. */
function cargoEdges(root) {
  let metadata;
  try {
    const output = execFileSync("cargo", ["metadata", "--format-version", "1", "--no-deps", "--offline"], {
      cwd: root,
      encoding: "utf8",
      maxBuffer: 64 * 1024 * 1024,
      stdio: ["ignore", "pipe", "pipe"],
    });
    metadata = JSON.parse(output);
  } catch (error) {
    return failedRun(
      "reading the Cargo workspace (`cargo metadata`)",
      error,
      "fix the Cargo.toml the message names (`cargo metadata --no-deps` reproduces it)",
    );
  }
  const shaped =
    Array.isArray(metadata?.packages) &&
    metadata.packages.every(
      (pkg) =>
        typeof pkg?.name === "string" &&
        typeof pkg.manifest_path === "string" &&
        Array.isArray(pkg.dependencies) &&
        pkg.dependencies.every(
          (dep) => typeof dep?.name === "string" && (dep.path === undefined || typeof dep.path === "string"),
        ),
    );
  if (!shaped) {
    failedRun(
      "reading the Cargo workspace (`cargo metadata`)",
      "its packages are not the shape this check reads",
      "check `cargo metadata --format-version 1 --no-deps` with this toolchain (rust-toolchain.toml)",
    );
  }
  return metadata.packages.flatMap((pkg) =>
    pkg.dependencies.filter((dep) => dep.path).map((dep) => [pkg.name, dep.name]),
  );
}

function main() {
  const root = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
  let nxJson;
  try {
    nxJson = JSON.parse(readFileSync(join(root, "nx.json"), "utf8"));
  } catch (error) {
    fail([`nx.json could not be read: ${error.message}`, "ACTION: restore a readable, valid nx.json"]);
  }
  const allow = nxJson?.boundaries?.allow;
  if (!wellFormedAllow(allow)) {
    fail([
      'nx.json has no well-formed "boundaries.allow" table to enforce',
      "ACTION: restore it: one `type:` tag per key, each listing the declared `type:` tags it may depend on",
    ]);
  }

  const graph = nxGraph(root);
  const nodes = Object.fromEntries(
    Object.entries(graph.nodes).map(([name, node]) => [name, { tags: node.data.tags ?? [] }]),
  );
  const requested = process.argv.slice(2);
  for (const name of requested) {
    if (!Object.hasOwn(nodes, name)) {
      fail([
        `no project named ${name} in the Nx graph (projects: ${Object.keys(nodes).sort().join(", ")})`,
        "ACTION: pass a listed project, or add a project.json naming it",
      ]);
    }
  }
  const checked = new Set(requested.length > 0 ? requested : Object.keys(nodes));
  const edges = Object.entries(graph.dependencies).flatMap(([source, deps]) =>
    deps.map((dep) => [source, dep.target]),
  );
  const problems = boundaryViolations({ nodes, edges, cargoEdges: cargoEdges(root), allow, checked });
  if (problems.length > 0) {
    fail([...problems, "ACTION: remove the edge, or change the boundary in nx.json deliberately and say why"]);
  }
  const scope = requested.length > 0 ? requested.join(", ") : `${checked.size} projects`;
  console.log(`check-project-boundaries: ${scope}: every edge allowed`);
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main();
