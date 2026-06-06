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
    external: ["electron"],
    logLevel: "info",
    write: !checkOnly,
  });
}
