import assert from "node:assert/strict";
import { createRequire } from "node:module";
import { mkdtemp, rm } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { build } from "esbuild";

const root = path.resolve(import.meta.dirname, "..");
const temp = await mkdtemp(path.join(os.tmpdir(), "cerul-reset-smoke-"));

try {
  const outfile = path.join(temp, "local-data-reset.cjs");
  await build({
    entryPoints: [path.join(root, "src", "local-data-reset.ts")],
    outfile,
    bundle: true,
    platform: "node",
    target: "node22",
    format: "cjs",
    logLevel: "silent",
  });

  const require = createRequire(import.meta.url);
  const reset = require(outfile);
  const paths = {
    data_dir: path.join(temp, "Application Support", "Cerul"),
    cache_dir: path.join(temp, "Application Support", "Cerul", "cache"),
    models_dir: path.join(temp, "Application Support", "Cerul", "models"),
    index_dir: path.join(temp, "Application Support", "Cerul", "indexes", "zvec"),
  };
  const safety = {
    homeDir: path.join(temp, "Home"),
    dataBaseDir: path.join(temp, "Application Support"),
  };

  const libraryTargets = reset.normalizeResetTargets(
    reset.localLibraryResetTargets(paths),
    safety,
  );
  assert.deepEqual(
    libraryTargets.map((target) => target.label),
    ["indexes", "cache", "endpoint", "pipelineJobsLog"],
  );
  assert.doesNotThrow(() =>
    reset.assertTargetsPreservePath(libraryTargets, paths.models_dir, "models"),
  );
  assert.equal(
    libraryTargets.some((target) => target.path === path.resolve(paths.models_dir)),
    false,
  );
  assert.equal(
    libraryTargets.some((target) => target.path === path.resolve(paths.data_dir)),
    false,
  );

  const externalTargets = reset.normalizeResetTargets(
    reset.localLibraryResetTargets(paths, path.join(temp, "ExternalMedia")),
    safety,
  );
  assert.equal(
    externalTargets.some(
      (target) =>
        target.label === "downloads" &&
        target.path === path.join(temp, "ExternalMedia", "sources"),
    ),
    true,
  );

  const inDataTargets = reset.normalizeResetTargets(
    reset.localLibraryResetTargets(paths, path.join(paths.data_dir, "Downloads")),
    safety,
  );
  assert.equal(
    inDataTargets.some(
      (target) =>
        target.label === "downloads" &&
        target.path === path.join(paths.data_dir, "Downloads", "sources"),
    ),
    true,
  );

  const previousDownloads = path.join(temp, "OldMedia", "sources");
  const previousTargets = reset.normalizeResetTargets(
    reset.localLibraryResetTargets(paths, null, [previousDownloads]),
    safety,
  );
  assert.equal(
    previousTargets.some(
      (target) => target.label === "downloads" && target.path === previousDownloads,
    ),
    true,
  );

  const factoryTargets = reset.normalizeResetTargets(
    reset.factoryResetTargets(paths, path.join(temp, "ElectronUserData"), true),
    safety,
  );
  assert.equal(
    factoryTargets.some((target) => target.path === path.resolve(paths.data_dir)),
    true,
  );
  assert.throws(
    () => reset.assertTargetsPreservePath(factoryTargets, paths.models_dir, "models"),
    /would delete preserved models/,
  );
  assert.throws(
    () =>
      reset.assertTargetsPreservePath(
        [{ label: "modelChild", path: path.join(paths.models_dir, "sources") }],
        paths.models_dir,
        "models",
      ),
    /would delete preserved models/,
  );

  assert.throws(
    () => reset.normalizeResetTargets([{ label: "home", path: safety.homeDir }], safety),
    /unsafe path/,
  );
} finally {
  await rm(temp, { recursive: true, force: true });
}
