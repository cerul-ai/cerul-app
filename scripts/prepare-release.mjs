#!/usr/bin/env node

import fs from "node:fs";
import path from "node:path";
import process from "node:process";

const version = process.argv[2]?.trim();
const dryRun = process.argv.includes("--dry-run");
const manifestPaths = [
  "package.json",
  "apps/desktop/package.json",
  "apps/electron-shell/package.json",
];

if (!version) {
  console.error("Usage: scripts/prepare-release.mjs <version> [--dry-run]");
  process.exit(1);
}

if (version.startsWith("v")) {
  console.error("Pass the package version without a leading v.");
  process.exit(1);
}

if (!/^\d+\.\d+\.\d+$/.test(version)) {
  console.error(`Invalid stable release version: ${version}`);
  process.exit(1);
}

const manifests = manifestPaths.map((file) => {
  const raw = fs.readFileSync(file, "utf8");
  return {
    file,
    data: JSON.parse(raw),
  };
});

const currentVersions = new Set(manifests.map(({ data }) => data.version));
if (currentVersions.size !== 1) {
  console.error(
    `Release manifests disagree on current version: ${[...currentVersions].join(", ")}`,
  );
  process.exit(1);
}

const currentVersion = [...currentVersions][0];
if (compareVersions(version, currentVersion) <= 0) {
  console.error(`Release version ${version} must be greater than current ${currentVersion}.`);
  process.exit(1);
}

for (const manifest of manifests) {
  manifest.data.version = version;
  if (!dryRun) {
    fs.writeFileSync(
      manifest.file,
      `${JSON.stringify(manifest.data, null, 2)}\n`,
      "utf8",
    );
  }
}

const files = manifestPaths.map((file) => path.relative(process.cwd(), file)).join(",");
console.log(
  `release_version status=${dryRun ? "validated" : "updated"} from=${currentVersion} to=${version} files=${files}`,
);

function compareVersions(left, right) {
  const a = parseVersion(left);
  const b = parseVersion(right);
  for (let index = 0; index < 3; index += 1) {
    if (a.core[index] !== b.core[index]) {
      return a.core[index] > b.core[index] ? 1 : -1;
    }
  }
  return comparePrerelease(a.prerelease, b.prerelease);
}

function parseVersion(value) {
  const [withoutBuild] = value.split("+", 1);
  const prereleaseStart = withoutBuild.indexOf("-");
  const coreVersion = prereleaseStart === -1 ? withoutBuild : withoutBuild.slice(0, prereleaseStart);
  const prerelease = prereleaseStart === -1 ? "" : withoutBuild.slice(prereleaseStart + 1);
  const core = coreVersion.split(".").map((part) => Number.parseInt(part, 10));
  return {
    core: [core[0] ?? 0, core[1] ?? 0, core[2] ?? 0],
    prerelease: prerelease ? prerelease.split(".") : [],
  };
}

function comparePrerelease(left, right) {
  if (left.length === 0 && right.length === 0) {
    return 0;
  }
  if (left.length === 0) {
    return 1;
  }
  if (right.length === 0) {
    return -1;
  }
  const length = Math.max(left.length, right.length);
  for (let index = 0; index < length; index += 1) {
    const a = left[index];
    const b = right[index];
    if (a === undefined) {
      return -1;
    }
    if (b === undefined) {
      return 1;
    }
    const numericA = /^\d+$/.test(a);
    const numericB = /^\d+$/.test(b);
    if (numericA && numericB) {
      const numberA = Number.parseInt(a, 10);
      const numberB = Number.parseInt(b, 10);
      if (numberA !== numberB) {
        return numberA > numberB ? 1 : -1;
      }
      continue;
    }
    if (numericA !== numericB) {
      return numericA ? -1 : 1;
    }
    if (a !== b) {
      return a > b ? 1 : -1;
    }
  }
  return 0;
}
