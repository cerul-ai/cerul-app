#!/usr/bin/env node
import { execFile } from "node:child_process";
import { readFile, writeFile } from "node:fs/promises";
import path from "node:path";
import process from "node:process";
import { promisify } from "node:util";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const manifestPath = path.join(root, "third-party", "yt-dlp-manifest.json");
const args = new Set(process.argv.slice(2));
const shouldUpdate = args.has("--update");
const dryRun = args.has("--dry-run");
const checkPinnedOnly = args.has("--check-pinned");
const execFileAsync = promisify(execFile);

const requiredAssets = [
  "yt-dlp.exe",
  "yt-dlp_linux",
  "yt-dlp_linux_aarch64",
  "yt-dlp_macos",
];

function fail(message) {
  console.error(message);
  process.exitCode = 1;
}

if (shouldUpdate && checkPinnedOnly) {
  fail("--update and --check-pinned cannot be used together");
  process.exit();
}

async function readManifest() {
  return JSON.parse(await readFile(manifestPath, "utf8"));
}

async function githubJson(url) {
  const token = process.env.GITHUB_TOKEN || process.env.GH_TOKEN || "";
  const response = await fetch(url, {
    headers: {
      Accept: "application/vnd.github+json",
      "User-Agent": "cerul-ytdlp-release-check",
      ...(token ? { Authorization: `Bearer ${token}` } : {}),
    },
  });
  if (!response.ok) {
    throw new Error(`GitHub request failed ${response.status}: ${url}`);
  }
  return response.json();
}

async function fetchText(url) {
  try {
    const response = await fetch(url, {
      headers: { "User-Agent": "cerul-ytdlp-release-check" },
      signal: AbortSignal.timeout(30000),
    });
    if (!response.ok) {
      throw new Error(`Download failed ${response.status}: ${url}`);
    }
    return response.text();
  } catch (error) {
    try {
      const { stdout } = await execFileAsync(
        "curl",
        ["-fsSL", "--retry", "3", "--retry-delay", "2", "--max-time", "60", url],
        { maxBuffer: 10 * 1024 * 1024 },
      );
      return stdout;
    } catch (curlError) {
      const fetchMessage = error instanceof Error ? error.message : String(error);
      const curlMessage = curlError instanceof Error ? curlError.message : String(curlError);
      throw new Error(`Download failed for ${url}: fetch=${fetchMessage}; curl=${curlMessage}`);
    }
  }
}

function parseChecksums(text) {
  const checksums = new Map();
  for (const line of text.split(/\r?\n/)) {
    const match = line.match(/^([a-fA-F0-9]{64})\s+\*?(.+)$/);
    if (match) {
      checksums.set(match[2].trim(), match[1].toLowerCase());
    }
  }
  return checksums;
}

function releaseAsset(release, name) {
  return release.assets?.find((asset) => asset.name === name) ?? null;
}

function validateManifestHashes(manifest, checksums) {
  const mismatches = [];
  for (const assetName of requiredAssets) {
    const expected = checksums.get(assetName);
    const actual = manifest.assets?.[assetName]?.sha256?.toLowerCase();
    if (!actual) {
      mismatches.push(`${assetName}: missing manifest hash`);
    } else if (actual !== expected) {
      mismatches.push(`${assetName}: manifest=${actual} upstream=${expected}`);
    }
  }
  if (mismatches.length > 0) {
    throw new Error(
      `Bundled yt-dlp manifest hashes do not match upstream SHA2-256SUMS:\n${mismatches.join("\n")}`,
    );
  }
}

function riskReport(manifest, release) {
  const keywords = (manifest.riskKeywords ?? []).map((keyword) => String(keyword).toLowerCase());
  const body = release.body ?? "";
  const lines = body.split(/\r?\n/);
  const matched = [];
  const samples = [];
  for (const keyword of keywords) {
    if (body.toLowerCase().includes(keyword)) {
      matched.push(keyword);
    }
  }
  for (const line of lines) {
    const clean = line.trim();
    if (!clean) {
      continue;
    }
    if (clean.startsWith("[![")) {
      continue;
    }
    const lower = clean.toLowerCase();
    if (keywords.some((keyword) => lower.includes(keyword))) {
      samples.push(clean.replace(/^[-*]\s+/, "").replace(/\s+/g, " "));
    }
    if (samples.length >= 12) {
      break;
    }
  }

  const report = [
    `# yt-dlp ${release.tag_name} risk scan`,
    "",
    `Release: ${release.html_url}`,
    `Published: ${release.published_at ?? "unknown"}`,
    "",
    matched.length > 0
      ? `Matched keywords: ${matched.join(", ")}`
      : "Matched keywords: none",
    "",
    "Review these notes when updating the bundled downloader:",
    ...(samples.length > 0 ? samples.map((line) => `- ${line}`) : ["- No configured risk keywords were found in the release notes."]),
    "",
  ].join("\n");
  return { matched, report };
}

async function appendStepSummary(report) {
  const summaryPath = process.env.GITHUB_STEP_SUMMARY;
  if (!summaryPath) {
    return;
  }
  await writeFile(summaryPath, `${report}\n`, { flag: "a" });
}

async function main() {
  const manifest = await readManifest();
  const repository = manifest.repository ?? "yt-dlp/yt-dlp";
  const release = await githubJson(
    checkPinnedOnly
      ? `https://api.github.com/repos/${repository}/releases/tags/${encodeURIComponent(manifest.version)}`
      : `https://api.github.com/repos/${repository}/releases/latest`,
  );
  const checksumAsset = releaseAsset(release, "SHA2-256SUMS");
  if (!checksumAsset) {
    throw new Error(`yt-dlp release ${release.tag_name} has no SHA2-256SUMS asset`);
  }

  const checksums = parseChecksums(await fetchText(checksumAsset.browser_download_url));
  const missingChecksums = requiredAssets.filter((asset) => !checksums.has(asset));
  if (missingChecksums.length > 0) {
    throw new Error(`SHA2-256SUMS is missing required assets: ${missingChecksums.join(", ")}`);
  }

  const { report } = riskReport(manifest, release);
  console.log(report);
  await appendStepSummary(report);

  const latestVersion = release.tag_name;
  if (shouldUpdate) {
    const nextManifest = {
      ...manifest,
      version: latestVersion,
      releaseUrl: release.html_url,
      releasePublishedAt: release.published_at,
      assets: Object.fromEntries(
        requiredAssets.map((assetName) => {
          const asset = releaseAsset(release, assetName);
          if (!asset) {
            throw new Error(`Latest yt-dlp release ${latestVersion} has no ${assetName} asset`);
          }
          return [
            assetName,
            {
              sha256: checksums.get(assetName),
              url: asset.browser_download_url,
            },
          ];
        }),
      ),
    };
    const serialized = `${JSON.stringify(nextManifest, null, 2)}\n`;
    if (dryRun) {
      console.log(`Would update ${path.relative(root, manifestPath)} to ${latestVersion}`);
    } else {
      await writeFile(manifestPath, serialized);
      console.log(`Updated ${path.relative(root, manifestPath)} to ${latestVersion}`);
    }
    return;
  }

  if (checkPinnedOnly) {
    if (release.tag_name !== manifest.version) {
      fail(
        `Bundled yt-dlp manifest version ${manifest.version} resolved to release ${release.tag_name}`,
      );
    } else {
      validateManifestHashes(manifest, checksums);
      console.log(`Bundled yt-dlp manifest hashes match pinned release: ${manifest.version}`);
    }
    return;
  }

  if (manifest.version !== latestVersion) {
    fail(
      `Bundled yt-dlp is stale: manifest has ${manifest.version}, latest stable is ${latestVersion}. Run: node scripts/check-ytdlp-release.mjs --update`,
    );
  } else {
    validateManifestHashes(manifest, checksums);
    console.log(`Bundled yt-dlp is current: ${manifest.version}`);
  }
}

main().catch((error) => {
  fail(error instanceof Error ? error.message : String(error));
});
