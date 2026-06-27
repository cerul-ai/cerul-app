import { access, chmod, copyFile, mkdir, readdir, rm, stat } from "node:fs/promises";
import { constants } from "node:fs";
import { execFile } from "node:child_process";
import { dirname, resolve } from "node:path";
import { promisify } from "node:util";

const execFileAsync = promisify(execFile);

const repoRoot = resolve(import.meta.dirname, "../../..");
const targetTriple = process.env.CERUL_TARGET_TRIPLE ?? "";
const executableSuffix = targetTriple.includes("windows") || process.platform === "win32" ? ".exe" : "";
const targetProfileDir = targetTriple
  ? resolve(repoRoot, "target", targetTriple, "release")
  : resolve(repoRoot, "target", "release");
const source = targetTriple
  ? resolve(targetProfileDir, `cerul-api${executableSuffix}`)
  : resolve(targetProfileDir, `cerul-api${executableSuffix}`);
const destination = resolve(
  repoRoot,
  "apps",
  "electron-shell",
  "bin",
  `cerul-core${executableSuffix}`,
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

const zvecLibrary = await findZvecRuntimeLibrary(targetProfileDir);
if (zvecLibrary) {
  const zvecDestination = resolve(dirname(destination), zvecLibrary.fileName);
  await copyFile(zvecLibrary.path, zvecDestination);
  if (process.platform !== "win32") {
    await chmod(zvecDestination, 0o755);
  }
  console.log(`staged ${zvecDestination}`);
  if (zvecLibrary.fileName.endsWith(".dylib")) {
    await ensureMacosLoaderRpath(destination);
  }
} else {
  throw new Error(`zvec runtime library was not found for ${targetTriple || process.platform} under ${targetProfileDir}`);
}

function zvecRuntimeLibraryName() {
  if (targetTriple) {
    if (targetTriple.includes("windows")) {
      return "zvec_c_api.dll";
    }
    if (targetTriple.includes("apple-darwin")) {
      return "libzvec_c_api.dylib";
    }
    if (targetTriple.includes("linux")) {
      return "libzvec_c_api.so";
    }
    throw new Error(`unsupported zvec runtime target triple: ${targetTriple}`);
  }
  if (process.platform === "win32") {
    return "zvec_c_api.dll";
  }
  if (process.platform === "darwin") {
    return "libzvec_c_api.dylib";
  }
  return "libzvec_c_api.so";
}

async function findZvecRuntimeLibrary(profileDir) {
  const fileName = zvecRuntimeLibraryName();
  for (const candidate of zvecRuntimeOverrideCandidates(fileName)) {
    if (await isFile(candidate)) {
      return { fileName, path: candidate };
    }
  }

  const direct = resolve(profileDir, fileName);
  const buildDir = resolve(profileDir, "build");
  let entries = [];
  try {
    entries = await readdir(buildDir, { withFileTypes: true });
  } catch (error) {
    if (error?.code !== "ENOENT") {
      throw error;
    }
  }
  const candidates = [];
  for (const entry of entries) {
    if (!entry.isDirectory() || !entry.name.startsWith("zvec-")) {
      continue;
    }
    const candidate = resolve(buildDir, entry.name, "out", "zvec-bundled", "lib", fileName);
    if (await isFile(candidate)) {
      candidates.push(candidate);
    }
  }
  const path = await newestFile(candidates);
  if (path) {
    return { fileName, path };
  }
  if (await isFile(direct)) {
    return { fileName, path: direct };
  }
  return null;
}

async function newestFile(paths) {
  let newest = null;
  for (const path of paths) {
    const metadata = await stat(path);
    if (!newest || metadata.mtimeMs > newest.mtimeMs) {
      newest = { path, mtimeMs: metadata.mtimeMs };
    }
  }
  return newest?.path ?? null;
}

function zvecRuntimeOverrideCandidates(fileName) {
  const candidates = [];
  if (process.env.ZVEC_LIB_DIR) {
    candidates.push(resolve(process.env.ZVEC_LIB_DIR, fileName));
  }
  if (process.env.ZVEC_ROOT) {
    candidates.push(resolve(process.env.ZVEC_ROOT, "lib", fileName));
    candidates.push(resolve(process.env.ZVEC_ROOT, "lib64", fileName));
  }
  return candidates;
}

async function isFile(path) {
  try {
    await access(path, constants.F_OK);
    return true;
  } catch {
    return false;
  }
}

async function ensureMacosLoaderRpath(binary) {
  if (process.platform !== "darwin") {
    return;
  }
  const { stdout } = await execFileAsync("otool", ["-l", binary]);
  if (stdout.includes("path @loader_path ")) {
    return;
  }
  await execFileAsync("install_name_tool", ["-add_rpath", "@loader_path", binary]);
  console.log(`added @loader_path rpath to ${binary}`);
}
