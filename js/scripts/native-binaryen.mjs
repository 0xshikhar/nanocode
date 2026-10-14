


import { execFileSync } from "node:child_process";
import { createHash, randomUUID } from "node:crypto";
import { existsSync, readFileSync } from "node:fs";
import { chmod, mkdir, readFile, rename, rm } from "node:fs/promises";
import { homedir } from "node:os";
import { join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const root = fileURLToPath(new URL("../../", import.meta.url));


export const binaryenReleases = {
  132: {
    "linux-x64": ["x86_64-linux", "195ddc94f9bc89f45abdabb0b9eea86023d727ba90eac8b35b80f2544fc30572"],
    "linux-arm64": ["aarch64-linux", "c58562417836c5d0493d89bdefc434933bdc097db641b483df86bcfa557a107f"],
    "darwin-arm64": ["arm64-macos", "98aad827847af7ef990ed7098d885725c8e5b5aae75073403635617ae4e259aa"],
    "darwin-x64": ["x86_64-macos", "40c3de90bb3766bd0282a895e139a6f50253dba49b4f5bb89e66faca162d832e"],
  },
};


export function pinnedBinaryenVersion(repository = root) {
  const pkg = JSON.parse(readFileSync(resolve(repository, "js/package.json"), "utf8"));
  const match = /^(\d+)\.0\.0$/.exec(pkg.devDependencies?.binaryen ?? "");
  if (!match) throw new Error("js/nanocodex devDependencies.binaryen must be an exact <version>.0.0 pin");
  return Number(match[1]);
}

export function nativeBinaryenRelease(version, platform = process.platform, arch = process.arch) {
  const entry = binaryenReleases[version]?.[platform + "-" + arch];
  if (!entry) return undefined;
  const [suffix, sha256] = entry;
  const directory = "binaryen-version_" + version;
  return {
    url: "https://github.com/WebAssembly/binaryen/releases/download/version_" + version + "/" + directory + "-" + suffix + ".tar.gz",
    sha256,
    directory,

    members: platform === "darwin" ? [directory + "/bin", directory + "/lib"] : [directory + "/bin/wasm-opt"],
  };
}

export function defaultCacheDirectory(environment = process.env) {
  return environment.NANOCODEX_BINARYEN_CACHE
    || join(environment.XDG_CACHE_HOME || join(homedir(), ".cache"), "nanocodex", "binaryen");
}


export async function resolveNativeWasmOpt({ repository = root, cache = defaultCacheDirectory(), fetchArchive = curl } = {}) {
  const version = pinnedBinaryenVersion(repository);
  const release = nativeBinaryenRelease(version);
  if (!release) throw new Error("no pinned native Binaryen " + version + " for " + process.platform + "-" + process.arch);
  const installed = join(cache, release.sha256, release.directory);
  const wasmOpt = join(installed, "bin", "wasm-opt");
  if (!existsSync(wasmOpt)) {


    const staging = join(cache, release.sha256 + "." + randomUUID() + ".tmp");
    await mkdir(staging, { recursive: true });
    try {
      const archive = join(staging, "binaryen.tar.gz");
      await fetchArchive(release.url, archive);
      const actual = createHash("sha256").update(await readFile(archive)).digest("hex");
      if (actual !== release.sha256) throw new Error("native Binaryen archive sha256 " + actual + " != pinned " + release.sha256);
      execFileSync("tar", ["-xzf", archive, "-C", staging, ...release.members], { stdio: ["ignore", "ignore", "inherit"] });
      await rm(archive);
      await chmod(join(staging, release.directory, "bin", "wasm-opt"), 0o755);
      await mkdir(join(cache, release.sha256), { recursive: true });
      try { await rename(join(staging, release.directory), installed); }
      catch (error) { if (!existsSync(wasmOpt)) throw error; }
    } finally { await rm(staging, { recursive: true, force: true }); }
  }
  const reported = execFileSync(wasmOpt, ["--version"], { encoding: "utf8" }).trim();
  if (reported !== "wasm-opt version " + version + " (version_" + version + ")") throw new Error("cached native Binaryen reports " + reported);
  return wasmOpt;
}

function curl(url, destination) {
  execFileSync("curl", ["--proto", "=https", "--tlsv1.2", "--fail", "--silent", "--show-error", "--location",
    "--retry", "2", "--connect-timeout", "10", "--max-time", "60", "--output", destination, url], { stdio: ["ignore", "ignore", "inherit"] });
}

if (process.argv[1] && pathToFileURL(resolve(process.argv[1])).href === import.meta.url) {
  try { console.log(await resolveNativeWasmOpt()); }
  catch (error) { console.error("native Binaryen unavailable: " + error.message); process.exit(1); }
}
