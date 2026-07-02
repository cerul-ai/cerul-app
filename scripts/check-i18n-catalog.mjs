#!/usr/bin/env node

import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, resolve } from "node:path";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const catalogPath = resolve(repoRoot, "apps/desktop/src/lib/i18n-catalog.ts");
const source = readFileSync(catalogPath, "utf8");

function extractCatalog(name) {
  const startPattern = new RegExp(`export const ${name}: Record<string, string> = \\{`);
  const startMatch = startPattern.exec(source);
  if (!startMatch) {
    throw new Error(`Missing ${name} catalog export`);
  }
  const bodyStart = startMatch.index + startMatch[0].length;
  const bodyEnd = source.indexOf("\n};", bodyStart);
  if (bodyEnd === -1) {
    throw new Error(`Could not find end of ${name} catalog`);
  }

  const body = source.slice(bodyStart, bodyEnd);
  const entries = new Map();
  const duplicates = [];
  const entryPattern = /^\s*"([^"]+)":\s*"((?:\\.|[^"\\])*)",\s*$/gm;
  let match;
  while ((match = entryPattern.exec(body)) !== null) {
    const [, key, rawValue] = match;
    if (entries.has(key)) {
      duplicates.push(key);
    }
    entries.set(key, rawValue);
  }
  return { entries, duplicates };
}

function varsFor(template) {
  return Array.from(template.matchAll(/\{([A-Za-z_][A-Za-z0-9_]*)\}/g), (match) => match[1]).sort();
}

const catalogs = {
  zh: extractCatalog("zh"),
  en: extractCatalog("en"),
};

const problems = [];
for (const [name, catalog] of Object.entries(catalogs)) {
  for (const key of catalog.duplicates) {
    problems.push(`${name}: duplicate key ${key}`);
  }
}

const zhKeys = new Set(catalogs.zh.entries.keys());
const enKeys = new Set(catalogs.en.entries.keys());

for (const key of zhKeys) {
  if (!enKeys.has(key)) {
    problems.push(`en: missing key ${key}`);
  }
}
for (const key of enKeys) {
  if (!zhKeys.has(key)) {
    problems.push(`zh: missing key ${key}`);
  }
}

for (const key of zhKeys) {
  if (!enKeys.has(key)) {
    continue;
  }
  const zhVars = varsFor(catalogs.zh.entries.get(key));
  const enVars = varsFor(catalogs.en.entries.get(key));
  if (zhVars.join(",") !== enVars.join(",")) {
    problems.push(
      `${key}: interpolation vars differ (zh: ${zhVars.join(",") || "none"}; en: ${enVars.join(",") || "none"})`,
    );
  }
}

if (!/export const catalogs: Record<Lang, Record<string, string>> = \{ zh, en \};/.test(source)) {
  problems.push("catalogs export must include exactly { zh, en }");
}

if (problems.length > 0) {
  console.error("i18n catalog check failed:");
  for (const problem of problems) {
    console.error(`- ${problem}`);
  }
  process.exit(1);
}

console.log(`i18n catalog check passed (${zhKeys.size} keys).`);
