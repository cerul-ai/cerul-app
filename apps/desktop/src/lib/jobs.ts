// Job and indexing-status display helpers. Extracted from App.tsx (B13 Phase E).
//
// Text-producing helpers take the active `t` (last argument, matching the
// providerStatusLabel convention in App.tsx) so the jobs sheet renders in the
// user's language. Unknown job types / stages fall back gracefully.

import { appLocaleTag } from "./i18n";
import type { TFunction } from "./i18n";
import type * as api from "./api";
import { formatDuration, formatUsd } from "./formatters";
import { formatEtaDuration, isActiveJob } from "./items";
import type { ApiStatus, Item } from "./types";

// Coarse, user-facing steps per pipeline. Each groups several backend stages and
// owns an equal slice of the progress bar, so "step N of M" and the bar fill
// always agree. `lo`/`hi` are the backend-progress range the step spans, used to
// interpolate the bar *within* a step (smooth fill) from the backend's
// time-weighted progress value.
type CoarseStep = { key: string; stages: string[]; lo: number; hi: number };

const COARSE_STEPS: Record<string, CoarseStep[]> = {
  index_video: [
    { key: "prepare", stages: ["processing", "fetching", "downloading", "extracting_audio", "sampling_frames", "preparing_models"], lo: 0, hi: 0.25 },
    { key: "transcribe", stages: ["transcribing", "chunking_transcript"], lo: 0.25, hi: 0.62 },
    { key: "embed_text", stages: ["ocr_frames", "writing_transcript", "embedding_text"], lo: 0.62, hi: 0.8 },
    { key: "embed_frames", stages: ["embedding_frames", "visual_failed"], lo: 0.8, hi: 0.92 },
    { key: "write_index", stages: ["writing_index", "partial", "completed"], lo: 0.92, hi: 1 },
  ],
  index_audio: [
    { key: "prepare", stages: ["processing", "fetching", "extracting_audio", "preparing_models"], lo: 0, hi: 0.25 },
    { key: "transcribe", stages: ["transcribing", "chunking_transcript", "writing_transcript"], lo: 0.25, hi: 0.68 },
    { key: "embed_text", stages: ["embedding_text"], lo: 0.68, hi: 0.92 },
    { key: "write_index", stages: ["writing_index", "partial", "completed"], lo: 0.92, hi: 1 },
  ],
};

function coarseStepIndex(job: api.JobRecord): number {
  const steps = COARSE_STEPS[job.job_type];
  if (!steps || !job.stage) {
    return -1;
  }
  return steps.findIndex((step) => step.stages.includes(job.stage as string));
}

// Stable key for the job's current coarse step (used to time how long the step
// has been running). Null for queued jobs or job types without a step model.
export function coarseStepKey(job: api.JobRecord): string | null {
  const steps = COARSE_STEPS[job.job_type];
  const index = coarseStepIndex(job);
  return index < 0 || !steps ? null : steps[index].key;
}

export function jobItemTitle(job: api.JobRecord, allItems: Item[], t: TFunction) {
  const item = allItems.find((candidate) => candidate.id === job.item_id);
  return item?.title ?? job.item_id ?? t("jobs.maintenance");
}

// Known job types (index_video/audio/image) get a localized label; anything
// else falls back to the humanized "some_type" → "some type".
export function jobTypeLabel(type: string, t: TFunction) {
  const key = `jobs.type.${type}`;
  const label = t(key);
  return label === key ? type.replaceAll("_", " ") : label;
}

export function jobBadgeStatus(status: string) {
  if (status === "failed" || status === "error") {
    return "failed";
  }
  if (status === "completed" || status === "done" || status === "cancelled" || status === "canceled") {
    return "indexed";
  }
  return "indexing";
}

export function jobStatusLabel(status: string, t: TFunction) {
  switch (status) {
    case "queued":
      return t("jobs.status.queued");
    case "running":
      return t("jobs.status.running");
    case "failed":
    case "error":
      return t("jobs.status.failed");
    case "completed":
    case "done":
      return t("jobs.status.completed");
    case "cancelled":
    case "canceled":
      return t("jobs.status.cancelled");
    default:
      return status;
  }
}

// Returns null for an unknown stage (no matching catalog key) so callers can
// fall back to the backend stage_message, preserving the original behaviour.
export function jobStageLabel(stage: string | null, t: TFunction): string | null {
  if (!stage) {
    return null;
  }
  const key = `jobs.stage.${stage}`;
  const label = t(key);
  return label === key ? null : label;
}

export function jobDisplayStatus(job: api.JobRecord, t: TFunction) {
  if (job.status === "running") {
    return jobStageLabel(job.stage, t) ?? job.stage_message ?? jobStatusLabel(job.status, t);
  }
  return jobStatusLabel(job.status, t);
}

// Prefer the localized stage label over the backend's English stage_message so
// the sheet stays in-language; fall back to stage_message for unknown stages.
// When the backend appended a "N/M" item count, keep it alongside the localized
// label (which would otherwise drop the English message and hide the count).
export function jobStageMessage(job: api.JobRecord, t: TFunction) {
  if (job.error) {
    return job.error;
  }
  const label = jobStageLabel(job.stage, t);
  if (label) {
    const count = stageCountSuffix(job.stage_message);
    return count ? `${label} · ${count}` : label;
  }
  return job.stage_message;
}

// Pulls a trailing "N/M" the backend appends to a stage message (e.g. the frame
// pass "Embedding visual frames · 47/111") so it can ride alongside the
// localized stage label. Returns null when there's no such suffix.
export function stageCountSuffix(message: string | null): string | null {
  if (!message) {
    return null;
  }
  const match = message.match(/(\d+)\s*\/\s*(\d+)\s*$/);
  return match ? `${match[1]}/${match[2]}` : null;
}

// Maps the active stage to "step N of M" within its pipeline. Returns null for
// queued/terminal stages or job types without a defined sequence, so callers
// can omit the chip rather than show a misleading step.
export function jobStepInfo(job: api.JobRecord): { current: number; total: number } | null {
  if (job.status !== "running") {
    return null;
  }
  const steps = COARSE_STEPS[job.job_type];
  const index = coarseStepIndex(job);
  return index < 0 || !steps ? null : { current: index + 1, total: steps.length };
}

// Step-based bar fill (0–100): each coarse step owns an equal slice; within the
// current step we interpolate from the backend's time-weighted progress mapped
// into the step's range. This keeps the bar and "step N/M" in agreement and
// still advances smoothly mid-step. Falls back to raw progress with no model.
export function jobStepProgressPercent(job: api.JobRecord): number {
  const raw = Math.min(Math.max(job.progress, 0), 1);
  const steps = COARSE_STEPS[job.job_type];
  const index = coarseStepIndex(job);
  if (index < 0 || !steps) {
    return Math.round(raw * 100);
  }
  const step = steps[index];
  const span = step.hi - step.lo;
  const intra = span > 0 ? Math.min(Math.max((raw - step.lo) / span, 0), 1) : 0;
  return Math.round(((index + intra) / steps.length) * 100);
}

// Seconds the job has spent in its *current* coarse step. `stepStarts` maps a
// job id to the wall-clock second its current step began (tracked app-side,
// since the backend only timestamps the whole job). Null when not running or
// unknown.
export function jobStepElapsedSeconds(
  job: api.JobRecord,
  stepStarts: Record<string, number>,
  nowSec: number,
): number | null {
  if (job.status !== "running") {
    return null;
  }
  const at = stepStarts[job.id];
  return at === undefined ? null : Math.max(0, nowSec - at);
}

// Wall-clock elapsed (or any duration) as a stopwatch string: "M:SS", or
// "H:MM:SS" once it crosses an hour.
export function formatClock(seconds: number): string {
  const total = Math.max(0, Math.floor(seconds));
  const hours = Math.floor(total / 3600);
  const minutes = Math.floor((total % 3600) / 60);
  const secs = total % 60;
  const mm = hours > 0 ? String(minutes).padStart(2, "0") : String(minutes);
  return hours > 0
    ? `${hours}:${mm}:${String(secs).padStart(2, "0")}`
    : `${mm}:${String(secs).padStart(2, "0")}`;
}

// Seconds a job has been running (live) or ran for (finished). `nowSec` is
// passed in so a ticking clock can drive live updates between data polls.
export function jobElapsedSeconds(job: api.JobRecord, nowSec: number): number | null {
  if (job.started_at === null) {
    return null;
  }
  const end = job.finished_at ?? nowSec;
  return Math.max(0, end - job.started_at);
}

// Rough remaining-time estimate from elapsed wall-clock and current progress.
// Progress is stage-weighted (not perfectly linear in time), so it's a "~".
// Suppressed below 12% (too little signal) and at/above 99% (basically done).
export function jobEtaLabel(job: api.JobRecord, nowSec: number, t: TFunction): string | null {
  if (job.status !== "running" || job.started_at === null) {
    return null;
  }
  const progress = Math.min(Math.max(job.progress, 0), 1);
  if (progress < 0.12 || progress >= 0.99) {
    return null;
  }
  const elapsed = nowSec - job.started_at;
  if (elapsed <= 1) {
    return null;
  }
  return t("item.eta.left", { duration: formatEtaDuration((elapsed * (1 - progress)) / progress) });
}

export function jobProgressPercent(job: api.JobRecord) {
  return Math.round(Math.min(Math.max(job.progress, 0), 1) * 100);
}

export function jobUsageLabel(job: api.JobRecord, t: TFunction) {
  const usage = job.usage;
  if (!usage || usage.event_count === 0) {
    return null;
  }
  const parts = [formatUsd(usage.estimated_usd)];
  if (usage.audio_seconds > 0) {
    parts.push(formatDuration(usage.audio_seconds));
  }
  if (usage.image_count > 0) {
    parts.push(
      t(usage.image_count === 1 ? "jobs.usage.images.one" : "jobs.usage.images.other", {
        count: usage.image_count,
      }),
    );
  }
  if (usage.input_tokens > 0) {
    parts.push(t("jobs.usage.inputTokens", { count: usage.input_tokens.toLocaleString(appLocaleTag()) }));
  }
  if (usage.unpriced_events > 0) {
    parts.push(t("jobs.usage.unpriced", { count: usage.unpriced_events }));
  }
  return parts.join(" · ");
}

export function sidebarStatusLabel(
  apiStatus: ApiStatus,
  items: Item[],
  jobs: api.JobRecord[],
  t: TFunction,
) {
  if (apiStatus !== "online") {
    return t("jobs.sidebar.offline");
  }
  const indexed = items.filter((item) => item.status === "indexed").length;
  const active = jobs.filter(isActiveJob).length;
  if (active > 0) {
    return t("jobs.sidebar.indexing", { active, indexed });
  }
  return indexed === 0 ? t("jobs.sidebar.idleEmpty") : t("jobs.sidebar.idle", { indexed });
}
