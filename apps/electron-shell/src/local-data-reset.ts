import path from "node:path";

export type StoragePaths = {
  data_dir: string;
  cache_dir: string;
  models_dir: string;
  index_dir: string;
};

export type ResetTarget = {
  label: string;
  path: string;
};

export type SafetyContext = {
  homeDir: string;
  dataBaseDir: string;
};

const endpointFileName = "endpoint.json";

export function localLibraryResetTargets(paths: StoragePaths, mediaDir?: string | null): ResetTarget[] {
  const targets = [
    { label: "indexes", path: path.join(paths.data_dir, "indexes") },
    { label: "cache", path: paths.cache_dir },
    { label: "endpoint", path: path.join(paths.data_dir, endpointFileName) },
    { label: "pipelineJobsLog", path: path.join(paths.data_dir, "logs", "pipeline-jobs.jsonl") },
  ];
  const externalDownloads = externalDownloadsTarget(paths, mediaDir);
  return externalDownloads ? [...targets, externalDownloads] : targets;
}

export function factoryResetTargets(
  paths: StoragePaths,
  userDataPath: string,
  isPackaged: boolean,
): ResetTarget[] {
  return [
    { label: "data", path: paths.data_dir },
    {
      label: isPackaged ? "userData" : "devStores",
      path: isPackaged ? userDataPath : path.join(userDataPath, "stores"),
    },
  ];
}

export function normalizeResetTargets(
  targets: ResetTarget[],
  safety: SafetyContext,
): ResetTarget[] {
  const seen = new Set<string>();
  return targets
    .map((target) => ({ ...target, path: path.resolve(target.path) }))
    .filter((target) => {
      if (seen.has(target.path)) {
        return false;
      }
      seen.add(target.path);
      return true;
    })
    .map((target) => {
      assertSafeResetTarget(target.path, safety);
      return target;
    });
}

export function assertSafeResetTarget(targetPath: string, safety: SafetyContext) {
  const resolved = path.resolve(targetPath);
  const forbidden = [
    path.parse(resolved).root,
    safety.homeDir,
    safety.dataBaseDir,
    path.dirname(safety.dataBaseDir),
  ].map((value) => path.resolve(value));
  if (forbidden.includes(resolved)) {
    throw new Error(`refusing to reset unsafe path: ${resolved}`);
  }
  const depth = resolved.split(path.sep).filter(Boolean).length;
  if (depth < 3) {
    throw new Error(`refusing to reset shallow path: ${resolved}`);
  }
}

export function assertTargetsPreservePath(
  targets: ResetTarget[],
  preservedPath: string,
  label: string,
) {
  const resolvedPreserved = path.resolve(preservedPath);
  const destructiveTarget = targets.find((target) =>
    targetRemovesPath(path.resolve(target.path), resolvedPreserved) ||
    targetRemovesPath(resolvedPreserved, path.resolve(target.path)),
  );
  if (destructiveTarget) {
    throw new Error(
      `refusing to reset ${destructiveTarget.path}; it would delete preserved ${label}: ${resolvedPreserved}`,
    );
  }
}

export function pathsMatch(left: string, right: string) {
  return path.resolve(left) === path.resolve(right);
}

function targetRemovesPath(targetPath: string, candidatePath: string) {
  const relative = path.relative(targetPath, candidatePath);
  return relative === "" || (!!relative && !relative.startsWith("..") && !path.isAbsolute(relative));
}

function externalDownloadsTarget(paths: StoragePaths, mediaDir?: string | null): ResetTarget | null {
  const cleaned = mediaDir?.trim();
  if (!cleaned) {
    return null;
  }
  const downloadsRoot = path.resolve(cleaned, "sources");
  const dataDir = path.resolve(paths.data_dir);
  if (downloadsRoot === dataDir || downloadsRoot.startsWith(`${dataDir}${path.sep}`)) {
    return null;
  }
  return { label: "downloads", path: downloadsRoot };
}
