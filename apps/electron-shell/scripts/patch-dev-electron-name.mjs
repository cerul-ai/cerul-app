// macOS reads the Dock label from the app bundle's Info.plist, so in dev mode
// the stock Electron binary always shows as "Electron" — app.setName() cannot
// change it at runtime. Patch CFBundleName/CFBundleDisplayName in the local
// node_modules Electron.app to "Cerul" and ad-hoc re-sign the bundle (editing
// Info.plist breaks the seal). Idempotent: exits fast once patched. No-op off
// macOS. Reinstalling node_modules restores the stock bundle, which is fine —
// the dev script re-runs this before every launch.
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { createRequire } from "node:module";

const APP_NAME = "Cerul";

if (process.platform !== "darwin") {
  process.exit(0);
}

const require = createRequire(import.meta.url);
const electronDist = path.join(
  path.dirname(require.resolve("electron/package.json")),
  "dist",
);
const appBundle = path.join(electronDist, "Electron.app");
const plist = path.join(appBundle, "Contents", "Info.plist");

if (!fs.existsSync(plist)) {
  console.warn(`[patch-dev-electron-name] Info.plist not found at ${plist}; skipping`);
  process.exit(0);
}

const read = (key) =>
  execFileSync("/usr/libexec/PlistBuddy", ["-c", `Print :${key}`, plist], {
    encoding: "utf8",
  }).trim();

const set = (key, value) => {
  try {
    execFileSync("/usr/libexec/PlistBuddy", ["-c", `Set :${key} ${value}`, plist]);
  } catch {
    execFileSync("/usr/libexec/PlistBuddy", ["-c", `Add :${key} string ${value}`, plist]);
  }
};

let currentDisplayName = "";
try {
  currentDisplayName = read("CFBundleDisplayName");
} catch {
  // key missing; fall through and add it
}

if (read("CFBundleName") === APP_NAME && currentDisplayName === APP_NAME) {
  process.exit(0);
}

set("CFBundleName", APP_NAME);
set("CFBundleDisplayName", APP_NAME);
// Re-sign so macOS keeps launching the modified bundle. Ad-hoc identity ("-")
// is enough for local dev.
execFileSync("codesign", ["--force", "--sign", "-", appBundle], { stdio: "inherit" });
console.log(`[patch-dev-electron-name] Renamed dev Electron bundle to "${APP_NAME}"`);
