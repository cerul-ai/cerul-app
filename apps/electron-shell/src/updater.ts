import { app, net } from "electron";

const defaultUpdateRepository = "cerul-ai/cerul-app";

type GitHubRelease = {
  tag_name?: string;
  name?: string | null;
  html_url?: string;
  body?: string | null;
  draft?: boolean;
  prerelease?: boolean;
  published_at?: string | null;
};

export type DesktopReleaseNotes = {
  publishedAt?: string;
  sections: Array<{
    title?: string;
    items: string[];
  }>;
};

export type DesktopUpdateInfo = {
  version: string;
  url: string;
  name?: string;
  prerelease: boolean;
  publishedAt?: string;
  releaseNotes?: DesktopReleaseNotes;
};

// Drives the rail "Update" pill. `available` always works (GitHub-release
// detection, signing-independent); later phases only occur once releases ship
// signed + a latest-mac.yml that electron-updater can apply.
export type UpdaterState =
  | { phase: "idle" }
  | {
      phase: "available";
      version: string;
      releaseUrl: string;
      canAutoInstall: boolean;
      releaseNotes?: DesktopReleaseNotes;
    }
  | {
      phase: "downloading";
      version: string;
      percent: number;
      bytesPerSecond?: number;
      etaSeconds?: number;
      transferredBytes?: number;
      totalBytes?: number;
      releaseNotes?: DesktopReleaseNotes;
    }
  | { phase: "preparing"; version: string; releaseNotes?: DesktopReleaseNotes }
  | { phase: "installing"; version: string; releaseNotes?: DesktopReleaseNotes }
  | { phase: "downloaded"; version: string; releaseNotes?: DesktopReleaseNotes }
  | {
      phase: "error";
      version?: string;
      message: string;
      releaseUrl: string;
      releaseNotes?: DesktopReleaseNotes;
    };

export type UpdaterProgress = {
  percent?: number;
  bytesPerSecond?: number;
  transferred?: number;
  total?: number;
};

export async function checkForGitHubReleaseUpdate(): Promise<DesktopUpdateInfo | null> {
  const repository = updateRepository();
  if (!/^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/.test(repository)) {
    throw new Error(`Invalid update repository: ${repository}`);
  }

  const currentVersion = normalizeVersion(app.getVersion());
  const updateChannel = process.env.CERUL_UPDATE_CHANNEL ?? "";
  const allowPrerelease = updateChannel === "alpha" || isPrereleaseVersion(currentVersion);
  const response = await net.fetch(`https://api.github.com/repos/${repository}/releases?per_page=20`, {
    headers: {
      Accept: "application/vnd.github+json",
      "User-Agent": `Cerul/${currentVersion}`,
    },
  });
  if (!response.ok) {
    throw new Error(`GitHub release check failed with HTTP ${response.status}`);
  }

  const releases = (await response.json()) as GitHubRelease[];
  let bestUpdate: DesktopUpdateInfo | null = null;
  for (const release of releases) {
    if (release.draft) {
      continue;
    }
    if (release.prerelease && !allowPrerelease) {
      continue;
    }
    const version = releaseVersionFromTag(release.tag_name);
    if (!version || !release.html_url || compareVersions(version, currentVersion) <= 0) {
      continue;
    }
    if (!bestUpdate || compareVersions(version, bestUpdate.version) > 0) {
      bestUpdate = {
        version,
        url: release.html_url,
        name: release.name ?? undefined,
        prerelease: Boolean(release.prerelease),
        publishedAt: release.published_at ?? undefined,
        releaseNotes: releaseNotesFromMarkdown(release.body, release.published_at),
      };
    }
  }
  return bestUpdate;
}

export function releasesPageUrl() {
  return `https://github.com/${updateRepository()}/releases`;
}

export function normalizeVersion(version: string) {
  return version.trim().replace(/^v/i, "");
}

export function isPrereleaseVersion(version: string) {
  return normalizeVersion(version).split("+", 1)[0].includes("-");
}

export function compareVersions(left: string, right: string) {
  const a = parseVersion(left);
  const b = parseVersion(right);
  for (let index = 0; index < 3; index += 1) {
    if (a.core[index] !== b.core[index]) {
      return a.core[index] > b.core[index] ? 1 : -1;
    }
  }
  return comparePrerelease(a.prerelease, b.prerelease);
}

function updateRepository() {
  return process.env.CERUL_UPDATE_REPOSITORY ?? defaultUpdateRepository;
}

function releaseVersionFromTag(tag: string | undefined) {
  if (!tag) {
    return null;
  }
  const version = normalizeVersion(tag);
  return /^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$/.test(version)
    ? version
    : null;
}

function releaseNotesFromMarkdown(
  markdown: string | null | undefined,
  publishedAt: string | null | undefined,
): DesktopReleaseNotes | undefined {
  const sections = releaseNoteSections(markdown ?? "");
  if (sections.length === 0) {
    return undefined;
  }
  return {
    publishedAt: publishedAt ?? undefined,
    sections,
  };
}

function releaseNoteSections(markdown: string): DesktopReleaseNotes["sections"] {
  const mainBody = markdown.split(/\n---\n/, 1)[0] ?? "";
  const sections: DesktopReleaseNotes["sections"] = [];
  let current: { title?: string; items: string[] } = { items: [] };

  function pushCurrent() {
    if (current.items.length > 0) {
      sections.push({
        title: current.title,
        items: current.items.slice(0, 8),
      });
    }
  }

  for (const rawLine of mainBody.split(/\r?\n/)) {
    const line = rawLine.trim();
    if (!line || line.startsWith("<!--")) {
      continue;
    }
    const heading = line.match(/^#{1,6}\s+(.+)$/);
    if (heading) {
      pushCurrent();
      current = { title: cleanReleaseNoteText(heading[1]), items: [] };
      continue;
    }
    const bullet = line.match(/^[-*]\s+(.+)$/);
    if (bullet) {
      const item = cleanReleaseNoteText(bullet[1]);
      if (item) {
        current.items.push(item);
      }
      continue;
    }
    if (sections.length === 0 && current.items.length === 0) {
      const item = cleanReleaseNoteText(line);
      if (item && !/^download:/i.test(item) && !/^github:/i.test(item)) {
        current.items.push(item);
      }
    }
  }
  pushCurrent();
  return sections.slice(0, 4);
}

function cleanReleaseNoteText(value: string) {
  return value
    .replace(/\[([^\]]+)\]\([^)]+\)/g, "$1")
    .replace(/[*_`~]/g, "")
    .replace(/\s+/g, " ")
    .trim();
}

function parseVersion(version: string) {
  const withoutBuild = normalizeVersion(version).split("+", 1)[0];
  const prereleaseStart = withoutBuild.indexOf("-");
  const coreVersion = prereleaseStart === -1 ? withoutBuild : withoutBuild.slice(0, prereleaseStart);
  const prerelease = prereleaseStart === -1 ? "" : withoutBuild.slice(prereleaseStart + 1);
  const core = coreVersion.split(".").map((part) => Number.parseInt(part, 10));
  return {
    core: [core[0] ?? 0, core[1] ?? 0, core[2] ?? 0],
    prerelease: prerelease ? prerelease.split(".") : [],
  };
}

function comparePrerelease(left: string[], right: string[]) {
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
