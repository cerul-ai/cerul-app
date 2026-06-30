#!/usr/bin/env node

import fs from "node:fs";
import process from "node:process";

const RELEASE_METADATA_FILES = [
  "README.md",
  "README.zh-CN.md",
  "apps/desktop/src/lib/i18n-catalog.ts",
];

const options = parseArgs(process.argv.slice(2));
const version = options.version ?? readPackageVersion();

if (!/^\d+\.\d+\.\d+$/.test(version)) {
  console.error(`Invalid stable release version: ${version}`);
  process.exit(1);
}

const changes = [];

syncFile("README.md", (text, file) => {
  let next = text;
  next = replaceRequired(
    next,
    /badge\/status-v\d+\.\d+\.\d+-/g,
    `badge/status-v${version}-`,
    file,
    "status badge",
  );
  next = replaceRequired(
    next,
    /Current version: \*\*\d+\.\d+\.\d+\*\*/g,
    `Current version: **${version}**`,
    file,
    "intro version",
  );
  next = replaceRequired(
    next,
    /Current release: \*\*\d+\.\d+\.\d+\*\*/g,
    `Current release: **${version}**`,
    file,
    "status version",
  );
  return next;
});

syncFile("README.zh-CN.md", (text, file) => {
  let next = text;
  next = replaceRequired(
    next,
    /badge\/status-v\d+\.\d+\.\d+-/g,
    `badge/status-v${version}-`,
    file,
    "status badge",
  );
  next = replaceRequired(
    next,
    /当前版本：\*\*\d+\.\d+\.\d+\*\*/g,
    `当前版本：**${version}**`,
    file,
    "intro version",
  );
  return replaceRequired(
    next,
    /当前版本：\*\*\d+\.\d+\.\d+\*\*/g,
    `当前版本：**${version}**`,
    file,
    "status version",
  );
});

syncFile("apps/desktop/src/lib/i18n-catalog.ts", (text, file) =>
  replaceRequired(
    text,
    /("settings\.about\.version\.fallback": ")\d+\.\d+\.\d+(")/g,
    `$1${version}$2`,
    file,
    "about fallback version",
  ),
);

if (changes.length > 0 && options.check) {
  console.error(`Release metadata is out of sync for ${version}:`);
  for (const file of changes) {
    console.error(`  ${file}`);
  }
  console.error("Run: node scripts/sync-release-metadata.mjs");
  process.exit(1);
}

const status = options.check
  ? "checked"
  : options.dryRun
    ? "would-update"
    : changes.length > 0
      ? "updated"
      : "unchanged";
console.log(
  `release_metadata status=${status} version=${version} files=${RELEASE_METADATA_FILES.join(",")}`,
);

function syncFile(file, update) {
  const original = fs.readFileSync(file, "utf8");
  const next = update(original, file);
  if (next === original) {
    return;
  }
  changes.push(file);
  if (!options.check && !options.dryRun) {
    fs.writeFileSync(file, next, "utf8");
  }
}

function replaceRequired(text, pattern, replacement, file, label) {
  const matches = text.match(pattern);
  if (!matches || matches.length === 0) {
    console.error(`Could not find ${label} in ${file}`);
    process.exit(1);
  }
  return text.replace(pattern, replacement);
}

function readPackageVersion() {
  const manifest = JSON.parse(fs.readFileSync("package.json", "utf8"));
  return manifest.version;
}

function parseArgs(args) {
  const parsed = {
    check: false,
    dryRun: false,
    version: undefined,
  };
  for (let index = 0; index < args.length; index += 1) {
    const arg = args[index];
    if (arg === "--check") {
      parsed.check = true;
    } else if (arg === "--dry-run") {
      parsed.dryRun = true;
    } else if (arg === "--version") {
      const value = args[index + 1];
      if (!value) {
        console.error("--version requires a value");
        process.exit(1);
      }
      parsed.version = value.trim().replace(/^v/, "");
      index += 1;
    } else if (arg === "--help" || arg === "-h") {
      console.log("Usage: scripts/sync-release-metadata.mjs [--version x.y.z] [--check|--dry-run]");
      process.exit(0);
    } else {
      console.error(`Unknown argument: ${arg}`);
      process.exit(1);
    }
  }
  return parsed;
}
