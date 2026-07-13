#!/usr/bin/env node
// Guards the desktop stylesheets against two recurring silent failures:
//
// 1. `var(--x)` referencing a custom property that no stylesheet defines.
//    An undefined var() invalidates the whole shorthand at computed-value
//    time with no warning — this shipped three times (--motion-fast,
//    --ease-out, --positive) before this check existed.
// 2. Font sizes below the 11px floor. The type contract is 12px minimum
//    for text; 11px is tolerated only for mono chips/badges. Anything
//    smaller is prototype debris. Lines annotated with `/* px-ok */`
//    are exempt (deliberate micro labels like keyboard hints).
//
// Usage: node scripts/check-css-tokens.mjs

import { readFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";

const STYLE_DIRS = ["apps/desktop/src/styles"];
const EXTRA_FILES = ["apps/desktop/src/styles.css"];
const FONT_FLOOR_PX = 11;

const files = [
  ...STYLE_DIRS.flatMap((dir) =>
    readdirSync(dir)
      .filter((name) => name.endsWith(".css"))
      .map((name) => join(dir, name)),
  ),
  ...EXTRA_FILES,
];

const sources = files.map((file) => ({ file, text: readFileSync(file, "utf8") }));

const defined = new Set();
for (const { text } of sources) {
  for (const match of text.matchAll(/(--[\w-]+)\s*:/g)) {
    defined.add(match[1]);
  }
}

// Some custom properties are injected at runtime from React style props
// (e.g. SplitStage sets --split-left). Count any `"--x":` in TS/TSX as a
// definition so those don't false-positive.
function walkTs(dir) {
  for (const name of readdirSync(dir)) {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) walkTs(path);
    else if (/\.tsx?$/.test(name)) {
      for (const match of readFileSync(path, "utf8").matchAll(/["'](--[\w-]+)["']\s*:/g)) {
        defined.add(match[1]);
      }
    }
  }
}
walkTs("apps/desktop/src");

const errors = [];

for (const { file, text } of sources) {
  const lines = text.split("\n");
  lines.forEach((line, index) => {
    const lineNo = index + 1;

    // var() with a fallback is self-healing; only bare references must resolve.
    for (const match of line.matchAll(/var\(\s*(--[\w-]+)\s*\)/g)) {
      if (!defined.has(match[1])) {
        errors.push(`${file}:${lineNo} undefined custom property ${match[1]}`);
      }
    }

    if (line.includes("px-ok")) {
      return;
    }
    // font-size: Npx  |  font: [weight] Npx[/lh] family
    for (const match of line.matchAll(/font(?:-size)?\s*:[^;]*?([\d.]+)px/g)) {
      const px = Number(match[1]);
      if (px > 0 && px < FONT_FLOOR_PX) {
        errors.push(`${file}:${lineNo} font size ${px}px is below the ${FONT_FLOOR_PX}px floor (annotate with /* px-ok */ only for deliberate micro labels)`);
      }
    }
  });
}

if (errors.length > 0) {
  console.error(`check-css-tokens: ${errors.length} problem(s)\n`);
  for (const error of errors) console.error(`  ${error}`);
  process.exit(1);
}
console.log(`check-css-tokens: ${files.length} stylesheets clean (var() refs resolve, font floor ${FONT_FLOOR_PX}px).`);
