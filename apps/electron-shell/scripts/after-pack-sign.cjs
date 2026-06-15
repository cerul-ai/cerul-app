// electron-builder afterPack hook.
//
// Two jobs on macOS:
//
// 1. The bundled MLX Python runtime (Contents/Resources/mlx-runtime) ships
//    hundreds of loose mach-O (.so/.dylib + the interpreter) that
//    `codesign --deep` does not reliably reach. We sign them inside-out so the
//    app's resource seal is valid and — when a Developer ID is configured —
//    the nested code is signed for notarization too.
//
// 2. When no Apple Developer ID is configured, electron-builder skips macOS
//    code signing and leaves the app with Electron's own partial (linker)
//    signature. On Apple Silicon that bundle fails `codesign --verify`, so
//    macOS treats a downloaded copy as "damaged". We re-sign the whole bundle
//    with a deep ad-hoc signature so its resources are sealed and it runs
//    locally (still unsigned for distribution / not notarized).
//
// When a real Developer ID IS provided, electron-builder signs the .app itself;
// we still sign the runtime above (NOTE: a notarized release also needs JIT
// entitlements on the interpreter — allow-jit, allow-unsigned-executable-memory,
// disable-library-validation — wire that when the Developer ID flow lands).
const { execFileSync } = require("node:child_process");
const fs = require("node:fs");
const path = require("node:path");

function collectMachO(dir) {
  const out = [];
  for (const name of fs.readdirSync(dir)) {
    const full = path.join(dir, name);
    const st = fs.lstatSync(full);
    if (st.isSymbolicLink()) continue; // sign the real file, not the symlink
    if (st.isDirectory()) out.push(...collectMachO(full));
    else if (/\.(so|dylib)$/.test(name) || name === "python3.12") out.push(full);
  }
  return out;
}

function chunk(arr, size) {
  const out = [];
  for (let i = 0; i < arr.length; i += size) out.push(arr.slice(i, i + size));
  return out;
}

exports.default = async function afterPack(context) {
  if (context.electronPlatformName !== "darwin") {
    return;
  }

  const appName = `${context.packager.appInfo.productFilename}.app`;
  const appPath = path.join(context.appOutDir, appName);
  const hasIdentity = !!(
    process.env.CSC_LINK ||
    process.env.APPLE_SIGNING_IDENTITY ||
    process.env.CSC_NAME
  );
  const identity = process.env.CSC_NAME || process.env.APPLE_SIGNING_IDENTITY || "-";

  // 1. Sign the bundled Python runtime's mach-O inside-out.
  const runtimeDir = path.join(appPath, "Contents", "Resources", "mlx-runtime");
  if (fs.existsSync(runtimeDir)) {
    const machO = collectMachO(runtimeDir);
    const args = ["--force", "--sign", identity];
    if (hasIdentity) args.push("--options", "runtime", "--timestamp");
    // execFileSync: no shell, so paths with $/backticks/spaces are safe.
    for (const batch of chunk(machO, 100)) {
      execFileSync("codesign", [...args, ...batch], { stdio: "inherit" });
    }
    console.log(`afterPack: signed ${machO.length} mach-O files in mlx-runtime`);
  }

  // 2. If a real signing identity is configured, let electron-builder own the
  // outer .app signature (the runtime is already signed above).
  if (hasIdentity) {
    return;
  }

  // No identity: deep ad-hoc sign the whole bundle so it isn't "damaged".
  execFileSync("codesign", ["--force", "--deep", "--sign", "-", appPath], {
    stdio: "inherit",
  });
  console.log(`afterPack: deep ad-hoc signed ${appName}`);
};
