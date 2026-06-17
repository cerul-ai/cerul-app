#!/usr/bin/env node
const { execFileSync, spawnSync } = require("node:child_process");
const fs = require("node:fs");
const path = require("node:path");

function collectMachO(dir) {
  const out = [];
  for (const name of fs.readdirSync(dir)) {
    const full = path.join(dir, name);
    const st = fs.lstatSync(full);
    if (st.isSymbolicLink()) continue;
    if (st.isDirectory()) out.push(...collectMachO(full));
    else if (/\.(so|dylib)$/.test(name) || name === "python3.12") out.push(full);
  }
  return out;
}

function prunePythonBytecode(dir) {
  let removed = 0;
  for (const name of fs.readdirSync(dir)) {
    const full = path.join(dir, name);
    const st = fs.lstatSync(full);
    if (st.isSymbolicLink()) continue;
    if (st.isDirectory()) {
      if (name === "__pycache__") {
        fs.rmSync(full, { recursive: true, force: true });
        removed += 1;
      } else {
        removed += prunePythonBytecode(full);
      }
    } else if (name.endsWith(".pyc") || name.endsWith(".pyo")) {
      fs.rmSync(full, { force: true });
      removed += 1;
    }
  }
  return removed;
}

function chunk(arr, size) {
  const out = [];
  for (let i = 0; i < arr.length; i += size) out.push(arr.slice(i, i + size));
  return out;
}

function isPythonInterpreter(file) {
  return path.basename(file) === "python3.12" && path.basename(path.dirname(file)) === "bin";
}

function runCodesign(args, options = {}) {
  return execFileSync("codesign", args, {
    encoding: "utf8",
    stdio: options.stdio ?? ["ignore", "pipe", "pipe"],
  });
}

function codesignDetails(file) {
  const result = spawnSync("codesign", ["-dv", file], { encoding: "utf8" });
  return String(result.stdout ?? "") + String(result.stderr ?? "");
}

function codesignEntitlements(file) {
  try {
    return runCodesign(["-d", "--entitlements", ":-", file]);
  } catch {
    return "";
  }
}

function hasRuntimeEntitlements(file) {
  const entitlements = codesignEntitlements(file);
  return (
    entitlements.includes("com.apple.security.cs.allow-jit") &&
    entitlements.includes("com.apple.security.cs.allow-unsigned-executable-memory") &&
    entitlements.includes("com.apple.security.cs.disable-library-validation")
  );
}

function validSignature(file, options) {
  if (options.force) return false;
  try {
    runCodesign(["--verify", "--strict", file]);
  } catch {
    return false;
  }

  const details = codesignDetails(file);
  if (options.hasIdentity) {
    const teamMatch = details.match(/TeamIdentifier=(.+)/);
    const team = teamMatch?.[1]?.trim();
    if (options.expectedTeamId) {
      if (team !== options.expectedTeamId) return false;
    } else if (!team || team === "not set") {
      return false;
    }
  }

  if (options.needsEntitlements && !hasRuntimeEntitlements(file)) {
    return false;
  }
  return true;
}

function signBatches(files, args, label) {
  for (const batch of chunk(files, 100)) {
    console.log(`sign_mlx_runtime signing ${batch.length} ${label}`);
    execFileSync("codesign", [...args, ...batch], { stdio: "inherit" });
  }
}

function signRuntime(options) {
  const runtimeDir = options.runtimeDir;
  if (!fs.existsSync(runtimeDir)) {
    return { status: "missing", signed: 0, skipped: 0, total: 0, prunedBytecode: 0 };
  }

  const prunedBytecode = options.pruneBytecode ? prunePythonBytecode(runtimeDir) : 0;
  const machO = collectMachO(runtimeDir);
  const interpreters = machO.filter(isPythonInterpreter);
  const libraries = machO.filter((file) => !isPythonInterpreter(file));
  const baseArgs = ["--force", "--sign", options.identity];
  if (options.hasIdentity) baseArgs.push("--options", "runtime", "--timestamp");

  const librariesToSign = libraries.filter(
    (file) => !validSignature(file, { ...options, needsEntitlements: false }),
  );
  const interpretersToSign = interpreters.filter(
    (file) => !validSignature(file, { ...options, needsEntitlements: options.hasIdentity }),
  );

  signBatches(librariesToSign, baseArgs, "runtime libraries");

  const interpreterArgs =
    options.hasIdentity && options.entitlements && fs.existsSync(options.entitlements)
      ? [...baseArgs, "--entitlements", options.entitlements]
      : baseArgs;
  signBatches(interpretersToSign, interpreterArgs, "runtime interpreters");

  const signed = librariesToSign.length + interpretersToSign.length;
  const skipped = machO.length - signed;
  console.log(
    `sign_mlx_runtime status=passed path=${runtimeDir} total=${machO.length} signed=${signed} skipped=${skipped} pruned_bytecode=${prunedBytecode}`,
  );
  return { status: "passed", signed, skipped, total: machO.length, prunedBytecode };
}

function parseArgs(argv) {
  const options = {
    runtimeDir: "",
    identity: process.env.APPLE_SIGNING_IDENTITY || process.env.CSC_NAME || "-",
    hasIdentity: Boolean(process.env.CSC_LINK || process.env.APPLE_SIGNING_IDENTITY || process.env.CSC_NAME),
    expectedTeamId: process.env.APPLE_TEAM_ID || "",
    entitlements: path.join(__dirname, "../entitlements/entitlements.mlx-runtime.plist"),
    pruneBytecode: true,
    force: process.env.CERUL_FORCE_MLX_RUNTIME_SIGNING === "1",
  };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === "--runtime-dir") {
      options.runtimeDir = argv[++i];
    } else if (arg === "--identity") {
      options.identity = argv[++i];
      options.hasIdentity = options.identity !== "-";
    } else if (arg === "--expected-team-id") {
      options.expectedTeamId = argv[++i];
    } else if (arg === "--entitlements") {
      options.entitlements = argv[++i];
    } else if (arg === "--no-prune-bytecode") {
      options.pruneBytecode = false;
    } else if (arg === "--force") {
      options.force = true;
    } else if (arg === "-h" || arg === "--help") {
      console.log("Usage: sign-mlx-runtime.cjs --runtime-dir <path> [--identity <name>] [--expected-team-id <id>] [--force]");
      process.exit(0);
    } else {
      throw new Error(`Unknown argument: ${arg}`);
    }
  }
  if (!options.runtimeDir) {
    throw new Error("--runtime-dir is required");
  }
  return options;
}

if (require.main === module) {
  signRuntime(parseArgs(process.argv.slice(2)));
}

module.exports = {
  signRuntime,
};
