// electron-builder afterPack hook.
//
// When no Apple Developer ID is configured, electron-builder skips macOS code
// signing and leaves the app with Electron's own partial (linker) signature.
// On Apple Silicon that bundle fails `codesign --verify` ("code has no
// resources but signature indicates they must be present"), so macOS treats a
// downloaded copy as "damaged" and offers to move it to the Trash.
//
// To avoid that, re-sign the whole bundle with a deep ad-hoc signature so its
// resources are sealed and it runs locally. The build is still unsigned for
// distribution (not notarized), so first launch still requires the user to
// bypass Gatekeeper — but the app is no longer reported as damaged.
//
// When a real Developer ID identity IS provided, electron-builder signs the app
// itself and this hook is a harmless no-op re-sign that we skip.
const { execSync } = require("node:child_process");
const path = require("node:path");

exports.default = async function afterPack(context) {
  if (context.electronPlatformName !== "darwin") {
    return;
  }

  // If a real signing identity is configured, let electron-builder own signing.
  if (process.env.CSC_LINK || process.env.APPLE_SIGNING_IDENTITY || process.env.CSC_NAME) {
    return;
  }

  const appName = `${context.packager.appInfo.productFilename}.app`;
  const appPath = path.join(context.appOutDir, appName);

  execSync(`codesign --force --deep --sign - ${JSON.stringify(appPath)}`, {
    stdio: "inherit",
  });
  console.log(`afterPack: deep ad-hoc signed ${appName}`);
};
