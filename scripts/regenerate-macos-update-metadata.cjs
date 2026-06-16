#!/usr/bin/env node

const crypto = require("node:crypto");
const fs = require("node:fs");
const path = require("node:path");
const { createRequire } = require("node:module");

const ROOT = path.resolve(__dirname, "..");

function usage() {
  console.log(`Usage: scripts/regenerate-macos-update-metadata.cjs [--bundle-root <path>] [--dry-run]

Regenerates macOS .blockmap files and latest-mac.yml from final artifact bytes.`);
}

function parseArgs(argv) {
  const args = {
    bundleRoot: path.join(ROOT, "target", "electron"),
    dryRun: false,
  };

  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === "--bundle-root") {
      args.bundleRoot = path.resolve(argv[++i] || "");
    } else if (arg === "--dry-run") {
      args.dryRun = true;
    } else if (arg === "-h" || arg === "--help") {
      usage();
      process.exit(0);
    } else {
      throw new Error(`Unknown argument: ${arg}`);
    }
  }

  return args;
}

function sha512Base64(file) {
  return crypto.createHash("sha512").update(fs.readFileSync(file)).digest("base64");
}

function yamlString(value) {
  return String(value).replace(/\\/g, "\\\\").replace(/'/g, "''");
}

async function main() {
  const { bundleRoot, dryRun } = parseArgs(process.argv.slice(2));
  const packageJson = JSON.parse(
    fs.readFileSync(path.join(ROOT, "apps", "electron-shell", "package.json"), "utf8"),
  );
  const version = packageJson.version;

  if (!fs.existsSync(bundleRoot)) {
    throw new Error(`Bundle root does not exist: ${bundleRoot}`);
  }

  const artifactNames = fs
    .readdirSync(bundleRoot)
    .filter((name) => /\.(zip|dmg)$/.test(name))
    .sort((a, b) => {
      const rank = (name) => (name.endsWith(".zip") ? 0 : name.endsWith(".dmg") ? 1 : 2);
      return rank(a) - rank(b) || a.localeCompare(b);
    });

  if (artifactNames.length === 0) {
    throw new Error(`No macOS zip or dmg artifacts found under ${bundleRoot}`);
  }

  if (dryRun) {
    for (const name of artifactNames) {
      console.log(`+ regenerate ${path.join(bundleRoot, `${name}.blockmap`)}`);
    }
    console.log(`+ write ${path.join(bundleRoot, "latest-mac.yml")}`);
    return;
  }

  const electronBuilderRequire = createRequire(
    require.resolve("electron-builder/package.json", {
      paths: [path.join(ROOT, "apps", "electron-shell")],
    }),
  );
  const { buildBlockMap } = electronBuilderRequire(
    "app-builder-lib/out/targets/blockmap/blockmap",
  );

  const files = [];
  for (const name of artifactNames) {
    const artifact = path.join(bundleRoot, name);
    const blockmap = `${artifact}.blockmap`;
    const updateInfo = await buildBlockMap(artifact, "gzip", blockmap);
    files.push({
      url: name,
      sha512: updateInfo.sha512 || sha512Base64(artifact),
      size: updateInfo.size || fs.statSync(artifact).size,
    });
  }

  const primary = files.find((file) => file.url.endsWith(".zip")) || files[0];
  const lines = [
    `version: ${version}`,
    "files:",
    ...files.flatMap((file) => [
      `  - url: ${file.url}`,
      `    sha512: ${file.sha512}`,
      `    size: ${file.size}`,
    ]),
    `path: ${primary.url}`,
    `sha512: ${primary.sha512}`,
    `releaseDate: '${yamlString(new Date().toISOString())}'`,
    "",
  ];

  const latestMacPath = path.join(bundleRoot, "latest-mac.yml");
  fs.writeFileSync(latestMacPath, lines.join("\n"));
  console.log(
    `macos_update_metadata status=regenerated latest=${latestMacPath} artifacts=${files.length}`,
  );
}

main().catch((error) => {
  console.error(error instanceof Error ? error.message : error);
  process.exit(1);
});
