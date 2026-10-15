import { execFileSync } from "node:child_process";
import { relative } from "node:path";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../", import.meta.url));
const metadata = JSON.parse(execFileSync("cargo", [
  "metadata", "--offline", "--locked", "--format-version", "1", "--no-deps",
], { cwd: root, encoding: "utf8" }));
const packages = new Map(metadata.packages.map(pkg => [pkg.name, pkg]));
const layer = name => relative(root, packages.get(name).manifest_path).split("/").slice(0, 2).join("/");
const forbidden = {
  "crates/core": ["crates/runtime", "crates/backends", "crates/integrations", "crates/nanocode"],
  "crates/runtime": ["crates/backends", "crates/integrations", "crates/nanocode"],
  "crates/integrations": ["crates/backends", "crates/nanocode"],
  "crates/backends": ["crates/nanocode"],
  "crates/tui": ["crates/core", "crates/runtime", "crates/backends", "crates/integrations", "crates/nanocode"],
};
const forbiddenDependencies = {
  "nanocode-tool-contracts": new Set([
    "nanocode-tools", "nanocode-tools-macros", "schemars", "proc-macro-crate", "async-trait",
  ]),
  "nanocode-store-jsonl": new Set(["rusqlite", "libsqlite3-sys", "nanocode-store-sqlite"]),
};
const backendImplementations = new Set([
  "nanocode-tool-runtime", "nanocode-mcp-tools", "nanocode-mcp-http",
]);

let checked = 0;
for (const name of packages.keys()) {
  for (const dep of packages.get(name).dependencies) {
    if (dep.kind !== "dev" && (dep.name === "symphonia" || dep.name.startsWith("symphonia-"))) {
      throw new Error(`Removed audio dependency: ${name} → ${dep.name}`);
    }
  }
  const banned = forbidden[layer(name)];
  if (!banned) continue;
  checked += 1;
  const visited = new Set();
  const pending = [[name]];
  while (pending.length) {
    const path = pending.pop();
    const current = path.at(-1);
    if (banned.includes(layer(current))) {
      throw new Error(`${layer(name)} crate depends on ${layer(current)}: ${path.join(" → ")}`);
    }
    if (layer(name) === "crates/backends" && backendImplementations.has(current)) {
      throw new Error(`Backend depends on host implementation: ${path.join(" → ")}`);
    }
    if (layer(name) === "crates/backends" && layer(current) === "crates/backends" && current !== name) {
      throw new Error(`Backend depends on another backend: ${path.join(" → ")}`);
    }
    if (visited.has(current)) continue;
    visited.add(current);
    for (const dep of packages.get(current).dependencies) {
      if (dep.kind === "dev") continue;
      if (forbiddenDependencies[name]?.has(dep.name)) {
        throw new Error(`Forbidden dependency: ${[...path, dep.name].join(" → ")}`);
      }
      if (packages.has(dep.name)) pending.push([...path, dep.name]);
    }
  }
}
console.log(`Layering verified for ${checked} core, runtime, integration and tui crates.`);
