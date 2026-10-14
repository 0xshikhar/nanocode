import { readFile, access } from "node:fs/promises";
const root = new URL("../", import.meta.url);
const manifest = JSON.parse(await readFile(new URL("package.json", root), "utf8"));
function paths(value) { return typeof value === "string" ? [value] : Object.values(value).flatMap(paths); }
for (const path of paths(manifest.exports)) await access(new URL(path, root));

for (const path of ["pkg-node/nanocodex.js", "pkg-web/nanocodex.js", "pkg-web/nanocodex_bg.js", "pkg-web/nanocodex_worker.js", "pkg-web/nanocodex_bg.wasm"]) await access(new URL(path, root));
console.log(`${manifest.name}: all public exports and WASM artifacts exist`);
