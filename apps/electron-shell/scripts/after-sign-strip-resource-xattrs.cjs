const { execFileSync } = require("node:child_process");
const fs = require("node:fs");
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
}

module.exports = afterSign;
module.exports.default = afterSign;
module.exports.stripDetachedCodeSignatureXattrs = stripDetachedCodeSignatureXattrs;
module.exports.verifyAppSignature = verifyAppSignature;
