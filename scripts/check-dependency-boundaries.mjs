import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../", import.meta.url));
const metadata = JSON.parse(execFileSync("cargo", [
  "metadata", "--format-version", "1", "--no-deps",
], { cwd: root, encoding: "utf8" }));
const packages = new Map(metadata.packages.map(pkg => [pkg.name, pkg]));
const shared = [
  "nanocode-agent", "nanocode-subagents", "nanocode-code-mode", "nanocode-cua",
  "nanocode-tools", "nanocode-types", "nanocode-media", "nanocode-tls",
];

for (const name of shared) {
  if (!packages.has(name)) throw new Error(`Missing shared crate: ${name}`);
  const visited = new Set();
  const pending = [[name]];
  while (pending.length) {
    const path = pending.pop();
    const current = path.at(-1);
    if (current.startsWith("nanocode-backend-")) {
      throw new Error(`Shared crate depends on a backend: ${path.join(" → ")}`);
    }
    if (visited.has(current)) continue;
    visited.add(current);
    for (const dep of packages.get(current)?.dependencies ?? []) {
      if (dep.kind !== "dev" && packages.has(dep.name)) pending.push([...path, dep.name]);
    }
  }
}
console.log(`Backend independence verified for ${shared.length} shared crates.`);
