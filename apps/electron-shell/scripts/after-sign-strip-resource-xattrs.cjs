const { execFileSync } = require("node:child_process");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");

function appPathFromContext(context) {
  const appName = `${context.packager.appInfo.productFilename}.app`;
  return path.join(context.appOutDir, appName);
}

function walkFiles(dir, visitor) {
  for (const name of fs.readdirSync(dir)) {
    const full = path.join(dir, name);
    const st = fs.lstatSync(full);
    if (st.isSymbolicLink()) continue;
    if (st.isDirectory()) {
      walkFiles(full, visitor);
    } else if (st.isFile()) {
      visitor(full);
    }
  }
}

function hasCodeSignatureXattr(file) {
  try {
    execFileSync("xattr", ["-p", "com.apple.cs.CodeSignature", file], {
      stdio: "ignore",
    });
    return true;
  } catch {
    return false;
  }
}

function stripDetachedCodeSignatureXattrs(appPath) {
  let scanned = 0;
  let stripped = 0;
  walkFiles(appPath, (file) => {
    scanned += 1;
    if (!hasCodeSignatureXattr(file)) return;
    execFileSync("xattr", ["-d", "com.apple.cs.CodeSignature", file], {
      stdio: "ignore",
    });
    stripped += 1;
  });
  return { scanned, stripped };
}

function verifyAppSignature(appPath) {
  execFileSync("codesign", ["--verify", "--deep", "--strict", "--verbose=2", appPath], {
    stdio: "inherit",
  });
}

function timingStart(name) {
  const start = Math.floor(Date.now() / 1000);
  console.log(`release_timing_start step=${name} epoch=${start}`);
  if (process.env.GITHUB_ACTIONS) {
    console.log(`::group::${name}`);
  }
  return start;
}

function timingEnd(name, start, status) {
  if (process.env.GITHUB_ACTIONS) {
    console.log("::endgroup::");
  }
  const end = Math.floor(Date.now() / 1000);
  console.log(`release_timing step=${name} seconds=${end - start} status=${status}`);
}

function runTimed(name, fn) {
  const start = timingStart(name);
  try {
    const result = fn();
    timingEnd(name, start, 0);
    return result;
  } catch (error) {
    timingEnd(name, start, error.status ?? 1);
    throw error;
  }
}

function notarizationCredentials() {
  const appleId = process.env.APPLE_ID || "";
  const password = process.env.APPLE_APP_SPECIFIC_PASSWORD || process.env.APPLE_PASSWORD || "";
  const teamId = process.env.APPLE_TEAM_ID || "";
  if (!appleId || !password || !teamId) {
    return null;
  }
  return { appleId, password, teamId };
}

function shouldNotarizeApp() {
  return (
    process.env.CERUL_NOTARIZE === "1" &&
    Boolean(process.env.CSC_LINK || process.env.APPLE_SIGNING_IDENTITY || process.env.CSC_NAME)
  );
}

function notarizeApp(appPath) {
  if (!shouldNotarizeApp()) {
    console.log("app_notarization status=skipped reason=not_configured");
    return;
  }

  const credentials = notarizationCredentials();
  if (!credentials) {
    throw new Error("APPLE_ID, APPLE_APP_SPECIFIC_PASSWORD, and APPLE_TEAM_ID are required for app notarization.");
  }

  const zipPath = path.join(os.tmpdir(), `cerul-app-notary-${process.pid}-${Date.now()}.zip`);
  try {
    execFileSync("ditto", ["-c", "-k", "--keepParent", appPath, zipPath], { stdio: "inherit" });
    execFileSync(
      "xcrun",
      [
        "notarytool",
        "submit",
        zipPath,
        "--apple-id",
        credentials.appleId,
        "--password",
        credentials.password,
        "--team-id",
        credentials.teamId,
        "--wait",
      ],
      { stdio: "inherit" },
    );
    execFileSync("xcrun", ["stapler", "staple", appPath], { stdio: "inherit" });
    execFileSync("xcrun", ["stapler", "validate", appPath], { stdio: "inherit" });
    console.log(`app_notarization status=passed app=${appPath}`);
  } finally {
    fs.rmSync(zipPath, { force: true });
  }
}

async function afterSign(context) {
  if (context.electronPlatformName !== "darwin") {
    return;
  }

  const appPath = appPathFromContext(context);
  const result = stripDetachedCodeSignatureXattrs(appPath);
  if (result.stripped > 0) {
    console.log(
      `afterSign: stripped ${result.stripped} detached com.apple.cs.CodeSignature xattrs from ${result.scanned} files`,
    );
  } else {
    console.log(`afterSign: no detached com.apple.cs.CodeSignature xattrs found in ${result.scanned} files`);
  }
  verifyAppSignature(appPath);
  runTimed("app_notarization", () => notarizeApp(appPath));
}

module.exports = afterSign;
module.exports.default = afterSign;
module.exports.stripDetachedCodeSignatureXattrs = stripDetachedCodeSignatureXattrs;
module.exports.verifyAppSignature = verifyAppSignature;
