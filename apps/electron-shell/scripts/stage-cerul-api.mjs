import { access, chmod, copyFile, mkdir, rm } from "node:fs/promises";
import { constants } from "node:fs";
import { dirname, resolve } from "node:path";

const repoRoot = resolve(import.meta.dirname, "../../..");
const targetTriple = process.env.CERUL_TARGET_TRIPLE ?? "";
const executableSuffix = targetTriple.includes("windows") || process.platform === "win32" ? ".exe" : "";
const source = targetTriple
  ? resolve(repoRoot, "target", targetTriple, "release", `cerul-api${executableSuffix}`)
  : resolve(repoRoot, "target", "release", `cerul-api${executableSuffix}`);
const destination = resolve(
  repoRoot,
  "apps",
  "electron-shell",
  "bin",
  `cerul-api${executableSuffix}`,
);

try {
  await access(source, constants.X_OK);
} catch {
  throw new Error(
    `release cerul-api binary is missing or not executable: ${source}. Run cargo build -p cerul-api --release first.`,
  );
}

await rm(dirname(destination), { recursive: true, force: true });
await mkdir(dirname(destination), { recursive: true });
await copyFile(source, destination);
await chmod(destination, 0o755);
console.log(`staged ${destination}`);
