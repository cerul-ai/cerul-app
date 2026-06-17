// electron-builder afterPack hook.
//
// Two jobs on macOS:
//
// 1. Older packaged builds copied the MLX Python runtime into
//    Contents/Resources/mlx-runtime as hundreds of loose mach-O files. Keep the
//    hook able to sign that shape for local/debug packaging. Public releases now
//    ship mlx-runtime.tar.gz and extract it on first launch to avoid Gatekeeper
//    recursively scanning the runtime inside the .app.
//
// 2. When no Apple Developer ID is configured, electron-builder skips macOS
//    code signing and leaves the app with Electron's own partial (linker)
//    signature. On Apple Silicon that bundle fails `codesign --verify`, so
//    macOS treats a downloaded copy as "damaged". We re-sign the whole bundle
//    with a deep ad-hoc signature so its resources are sealed and it runs
//    locally (still unsigned for distribution / not notarized).
//
// When a real Developer ID IS provided, electron-builder signs the .app itself;
// we still sign the runtime above and attach JIT entitlements to the embedded
// Python interpreter so the local MLX process can run under hardened runtime.
const fs = require("node:fs");
const path = require("node:path");
const {
  stripDetachedCodeSignatureXattrs,
  verifyAppSignature,
} = require("./after-sign-strip-resource-xattrs.cjs");
const { execFileSync } = require("node:child_process");
const { signRuntime } = require("./sign-mlx-runtime.cjs");

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
  const identity = process.env.APPLE_SIGNING_IDENTITY || process.env.CSC_NAME || "-";

  // 1. Sign the bundled Python runtime's mach-O inside-out.
  const runtimeDir = path.join(appPath, "Contents", "Resources", "mlx-runtime");
  if (fs.existsSync(runtimeDir)) {
    const result = signRuntime({
      runtimeDir,
      identity,
      hasIdentity,
      expectedTeamId: process.env.APPLE_TEAM_ID || "",
      entitlements: path.join(__dirname, "../entitlements/entitlements.mlx-runtime.plist"),
      pruneBytecode: true,
      force: process.env.CERUL_FORCE_MLX_RUNTIME_SIGNING === "1",
    });
    console.log(
      `afterPack: mlx-runtime signatures total=${result.total} signed=${result.signed} skipped=${result.skipped}`,
    );
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
  const result = stripDetachedCodeSignatureXattrs(appPath);
  if (result.stripped > 0) {
    console.log(`afterPack: stripped ${result.stripped} detached com.apple.cs.CodeSignature xattrs`);
  }
  verifyAppSignature(appPath);
  console.log(`afterPack: deep ad-hoc signed ${appName}`);
};
