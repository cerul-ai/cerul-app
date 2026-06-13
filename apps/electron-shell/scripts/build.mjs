import { build } from "esbuild";
import { rm } from "node:fs/promises";
import { resolve } from "node:path";

const root = resolve(import.meta.dirname, "..");
const checkOnly = process.argv.includes("--check");

if (!checkOnly) {
  await rm(resolve(root, "dist"), { recursive: true, force: true });
}

for (const entry of ["main", "preload"]) {
  await build({
    entryPoints: [resolve(root, "src", `${entry}.ts`)],
    outfile: resolve(root, "dist", `${entry}.js`),
    bundle: true,
    platform: "node",
    target: "node22",
    format: "cjs",
    // electron-updater is lazy-required at runtime (see getAutoUpdater in
    // main.ts) and resolved from node_modules in the packaged app; keeping it
    // external avoids bundling its dynamic requires.
    external: ["electron", "electron-updater"],
    logLevel: "info",
    write: !checkOnly,
  });
}
