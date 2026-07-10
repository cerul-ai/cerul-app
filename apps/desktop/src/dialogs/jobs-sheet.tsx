import { useRef } from "react";
// Tasks panel — a filterable timeline of indexing jobs (running / done /
// failed). Restyled to the FGH React/Tailwind prototype (§H). Extracted from
// App.tsx (B13 Phase D).

import { Check, Pause, Play, Trash2, X } from "lucide-react";
import * as api from "../lib/api";
import { useT } from "../lib/i18n";
import { isActiveJob } from "../lib/items";
import {
  formatClock,
  jobBadgeStatus,
  jobDisplayStatus,
  jobElapsedSeconds,
  jobEtaLabel,
  jobItemTitle,
  jobStageMessage,
  jobStepElapsedSeconds,
  jobStepInfo,
  jobStepProgressPercent,
  jobTypeLabel,
  jobUsageLabel,
} from "../lib/jobs";
import type { Item, Source } from "../lib/types";
import { useDialogFocus, useEscapeToClose } from "../lib/use-dismissable";
import { useNowSeconds } from "../lib/use-now";
import { ItemModalityIcon } from "../components/cards";
import { EmptyState } from "../components/leaf";
import { ProgressBar } from "../components/transcript";

type JobGroup = "running" | "done" | "failed";
export type JobsFilter = "all" | JobGroup;

function jobGroup(job: api.JobRecord): JobGroup {
  const badge = jobBadgeStatus(job.status);
  if (badge === "failed") return "failed";
  if (badge === "indexed") return "done";
  return "running";
}

function jobSortTime(job: api.JobRecord): number {
  return isActiveJob(job) ? job.started_at ?? job.finished_at ?? 0 : job.finished_at ?? job.started_at ?? 0;
}

export function JobsSheet({
  jobs,
  summary,
  filter,
  loading = false,
  syncingSources = [],
  items,
  stepStarts,
  paused = false,
  controlsEnabled = true,
  onTogglePause,
  onFilterChange,
  onCancelJob,
  onCancelQueuedJobs,
  onClose,
  onOpenSettingsFix,
  onOpenSources,
}: {
  jobs: api.JobRecord[];
  summary: api.JobStatusSummary | null;
  filter: JobsFilter;
  loading?: boolean;
  syncingSources?: Source[];
  items: Item[];
  stepStarts: Record<string, number>;
  paused?: boolean;
  controlsEnabled?: boolean;
  onTogglePause?: () => void;
  onFilterChange: (filter: JobsFilter) => void;
  onCancelJob?: (job: api.JobRecord) => void;
  onCancelQueuedJobs?: () => void;
  onClose: () => void;
  onOpenSettingsFix: (section: string) => void;
  onOpenSources?: () => void;
}) {
  const t = useT();
  useEscapeToClose(onClose);
  const dialogRef = useRef<HTMLElement | null>(null);
  useDialogFocus(dialogRef);

  const sortedJobs = [...jobs].sort((a, b) => {
    const activeDelta = Number(isActiveJob(b)) - Number(isActiveJob(a));
    if (activeDelta !== 0) {
      return activeDelta;
    }
    return jobSortTime(b) - jobSortTime(a);
  });
  const activeJobs = sortedJobs.filter(isActiveJob);
  const queuedJobs = activeJobs.filter((job) => job.status === "queued");
  const runningJobs = activeJobs.filter((job) => job.status === "running");
  const queuedCount = summary?.queued_jobs ?? queuedJobs.length;
  const runningCount = summary?.running_jobs ?? runningJobs.length;
  const activeCount = queuedCount + runningCount + syncingSources.length;
  const totalCount = (summary?.total_jobs ?? sortedJobs.length) + syncingSources.length;
  const onlyPausedQueuedJobs =
    paused && syncingSources.length === 0 && runningCount === 0 && queuedCount > 0;
  const failedJobs = sortedJobs.filter((job) => jobGroup(job) === "failed");
  const doneJobs = sortedJobs.filter((job) => jobGroup(job) === "done");
  const failedCount = summary?.failed_jobs ?? failedJobs.length;
  const doneCount = summary ? summary.completed_jobs + summary.cancelled_jobs : doneJobs.length;
  const hasAnyJobSignal = totalCount > 0;
  const now = useNowSeconds(activeCount > 0);
  const itemForJob = (job: api.JobRecord) =>
    job.item_id ? items.find((item) => item.id === job.item_id) ?? null : null;
  const focusJob = activeJobs[0] ?? doneJobs[0] ?? failedJobs[0] ?? sortedJobs[0] ?? null;
  const focusItem = focusJob ? itemForJob(focusJob) : null;

  const filters: { id: JobsFilter; label: string; n: number }[] = [
    { id: "all", label: t("jobs.filter.all"), n: totalCount },
    { id: "running", label: t(paused ? "jobs.groupQueued" : "jobs.groupRunning"), n: activeCount },
    { id: "done", label: t("jobs.status.completed"), n: doneCount },
    { id: "failed", label: t("jobs.status.failed"), n: failedCount },
  ];
  const showFilterControls = hasAnyJobSignal || filter !== "all" || loading;
  const visibleSyncingSources = filter === "all" || filter === "running" ? syncingSources : [];

  // Header summary, with the failed count tinted red (prototype §H).
  const runningLabel =
    onlyPausedQueuedJobs
      ? t(queuedCount === 1 ? "jobs.queuedCountOne" : "jobs.queuedCountOther", {
          count: queuedCount,
        })
      : activeCount > 0
      ? t(activeCount === 1 ? "jobs.activeCountOne" : "jobs.activeCountOther", {
          count: activeCount,
        })
      : null;
  const subtitle =
    hasAnyJobSignal
      ? t("jobs.subtitle", {
          done: doneCount,
          failed: failedCount,
          running: activeCount,
        })
      : t("jobs.emptyBody");
  const emptyTitle =
    filter === "failed"
      ? t("jobs.emptyFailedTitle")
      : filter === "running"
        ? t("jobs.emptyRunningTitle")
        : t("jobs.noneTitle");
  const emptyBody =
    filter === "failed"
      ? t("jobs.emptyFailedBody")
      : filter === "running"
        ? t("jobs.emptyRunningBody")
        : t("jobs.emptyBody");

  const renderJobThumb = (job: api.JobRecord) => {
    const item = itemForJob(job);
    return (
      <span
        className={`jobs-media-thumb ${item?.thumbnailUrl ? "has-image" : item?.color ?? "steel"}`}
        aria-hidden="true"
      >
        {item?.thumbnailUrl ? (
          <img src={item.thumbnailUrl} alt="" loading="lazy" />
        ) : item ? (
          <ItemModalityIcon item={item} size={20} />
        ) : (
          <Play size={20} fill="currentColor" />
        )}
        {item?.duration && item.contentType !== "image" ? (
          <small className="mono">{item.duration}</small>
        ) : null}
      </span>
    );
  };

  const renderSyncingSourceCard = (source: Source) => (
    <article className="job-card-v2 steel" key={`source:${source.id}`}>
      <span className="jobs-media-thumb steel" aria-hidden="true">
        <Play size={20} fill="currentColor" />
      </span>
      <div className="job-card-copy">
        <strong className="job-card-title clamp1">{source.name}</strong>
        <span className="job-card-type clamp1">{t("jobs.type.source_discovery")}</span>
        <p className="muted job-card-stage">{t("jobs.sourceDiscovery.body")}</p>
      </div>
      <span className="job-status-pill steel">
        <span className="jobs-tl-pill-dot pulse" />
        {t("jobs.status.discovering")}
      </span>
    </article>
  );

  const renderCard = (job: api.JobRecord) => {
    const group = jobGroup(job);
    const tone = group === "failed" ? "danger" : group === "done" ? "success" : "steel";
    const isRunning = job.status === "running";
    const isFailed = group === "failed";
    const isDone = group === "done";
    const stage = jobStageMessage(job, t);
    const usage = jobUsageLabel(job, t);
    const step = jobStepInfo(job);
    const stepElapsed = jobStepElapsedSeconds(job, stepStarts, now);
    const elapsed = jobElapsedSeconds(job, now);
    const fixSection = job.error_info?.settings_section?.trim() || null;
    const meta = [
      step ? t("jobs.step", { current: step.current, total: step.total }) : null,
      stepElapsed !== null ? t("jobs.stepElapsed", { duration: formatClock(stepElapsed) }) : null,
      elapsed !== null ? t("jobs.elapsed", { duration: formatClock(elapsed) }) : null,
      jobEtaLabel(job, now, t),
    ].filter(Boolean);
    const typeLine = [
      jobTypeLabel(job.job_type, t),
      isDone && elapsed !== null ? t("jobs.elapsed", { duration: formatClock(elapsed) }) : null,
    ]
      .filter(Boolean)
      .join(" · ");
    const canCancel = onCancelJob && controlsEnabled && job.item_id && (isRunning || job.status === "queued");

    return (
      <article className={`job-card-v2 ${tone}`} key={job.id}>
        {renderJobThumb(job)}
        <div className="job-card-copy">
          <div className="job-card-head">
            <strong className="job-card-title clamp1">{jobItemTitle(job, items, t)}</strong>
          </div>

          <div className="job-card-type clamp1">{typeLine}</div>

          {isRunning ? (
            <div className="job-card-progress">
              <ProgressBar value={jobStepProgressPercent(job)} animated />
              <span className="job-card-pct mono">{jobStepProgressPercent(job)}%</span>
            </div>
          ) : null}

          {isFailed && job.error_info ? (
            <div className="job-fix">
              <p>
                {t(`jobs.error.${job.error_info.code}`, {
                  capability: jobTypeLabel(job.job_type, t),
                })}
              </p>
              {job.error_info.code === "source_unavailable" ? (
                onOpenSources ? (
                  <button type="button" className="btn btn-primary sm" onClick={onOpenSources}>
                    {t("jobs.viewSources")}
                  </button>
                ) : null
              ) : job.error_info.code === "unknown_processing_error" || !fixSection ? null : (
                <button
                  type="button"
                  className="btn btn-primary sm"
                  onClick={() => onOpenSettingsFix(fixSection)}
                >
                  {t("jobs.fixSettings")}
                </button>
              )}
            </div>
          ) : null}

          {!isFailed && !isRunning && stage ? <p className="muted job-card-stage">{stage}</p> : null}

          {meta.length > 0 && !isDone ? (
            <p className="job-meta faint mono">{meta.join(" · ")}</p>
          ) : null}
          {isDone && usage ? <p className="job-usage faint mono">{usage}</p> : null}

          {isFailed && job.error ? (
            <details className="job-tech">
              <summary>{t("jobs.tech.summary")}</summary>
              <pre className="job-tech-raw mono">{job.error}</pre>
            </details>
          ) : null}
        </div>
        <div className="job-card-actions">
          <span className={`job-status-pill ${tone}`}>
              {isDone ? (
                <Check size={11} />
              ) : (
                <span className={`jobs-tl-pill-dot ${isRunning ? "pulse" : ""}`} />
              )}
              {jobDisplayStatus(job, t)}
          </span>
          {canCancel ? (
            <button
              type="button"
              className="btn-icon sm job-cancel"
              aria-label={t("jobs.cancelAria")}
              title={t("jobs.cancelAria")}
              onClick={() => onCancelJob?.(job)}
            >
              <Trash2 size={13} />
            </button>
          ) : null}
        </div>
      </article>
    );
  };

  return (
    <div className="scrim sheet-backdrop jobs-center-backdrop" role="presentation" onMouseDown={onClose}>
      <aside
        ref={dialogRef}
        className="jobs-center jobs-sheet"
        role="dialog"
        aria-modal="true"
        aria-labelledby="jobs-title"
        onMouseDown={(event) => event.stopPropagation()}
      >
        <header className="jobs-center-head dialog-header">
          <div className="grow">
            <p className="section-label eyebrow">{t("jobs.eyebrow")}</p>
            <h2 id="jobs-title" className="jobs-center-title">
              {runningLabel ? <span>{runningLabel}</span> : null}
              {runningLabel && failedCount > 0 ? <span> · </span> : null}
              {failedCount > 0 ? (
                <span className="jobs-title-failed">{t("jobs.failedCount", { count: failedCount })}</span>
              ) : null}
              {!runningLabel && failedCount === 0 ? <span>{t("jobs.noneTitle")}</span> : null}
            </h2>
            <p className="jobs-center-subtitle">{subtitle}</p>
          </div>
          {onTogglePause && controlsEnabled ? (
            <button
              type="button"
              className={paused ? "btn btn-primary sm jobs-pause" : "btn btn-secondary sm jobs-pause"}
              onClick={onTogglePause}
            >
              {paused ? <Play size={14} /> : <Pause size={14} />}
              <span>{paused ? t("jobs.resume") : t("jobs.pause")}</span>
            </button>
          ) : null}
          <button className="btn-icon" type="button" aria-label={t("jobs.closeAria")} onClick={onClose}>
            <X size={17} />
          </button>
        </header>

        <div className="jobs-summary-row">
          <div className="jobs-summary-card">
            <span className="jobs-summary-label">
              <span>{t("jobs.summary.total")}</span>
              <span>{t("jobs.summary.recent")}</span>
            </span>
            <strong className="jobs-summary-value mono">{totalCount}</strong>
          </div>
          <div className="jobs-summary-card">
            <span className="jobs-summary-label">
              <span>{t(paused ? "jobs.groupQueued" : "jobs.groupRunning")}</span>
              <span>{activeCount > 0 ? t("jobs.summary.live") : t("jobs.summary.idle")}</span>
            </span>
            <strong className="jobs-summary-value mono">{activeCount}</strong>
          </div>
          <div className="jobs-summary-card success">
            <span className="jobs-summary-label">
              <span>{t("jobs.status.completed")}</span>
              <span>{t("jobs.summary.done")}</span>
            </span>
            <strong className="jobs-summary-value mono">{doneCount}</strong>
          </div>
          <div className={failedCount > 0 ? "jobs-summary-card danger" : "jobs-summary-card"}>
            <span className="jobs-summary-label">
              <span>{t("jobs.status.failed")}</span>
              <span>{failedCount > 0 ? t("jobs.summary.needsFix") : t("jobs.summary.clear")}</span>
            </span>
            <strong className="jobs-summary-value mono">{failedCount}</strong>
          </div>
        </div>

        <div className="jobs-center-body">
          <aside className="jobs-focus-panel" aria-label={t("jobs.focus.aria")}>
            <p className="jobs-focus-label">{t("jobs.focus.title")}</p>
            {focusJob ? (
              <article className="jobs-focus-card">
                {renderJobThumb(focusJob)}
                <div className="jobs-focus-copy">
                  <strong className="clamp1">{jobItemTitle(focusJob, items, t)}</strong>
                  <span className="muted clamp1">
                    {jobTypeLabel(focusJob.job_type, t)}
                    {jobElapsedSeconds(focusJob, now) !== null
                      ? ` · ${t("jobs.elapsed", { duration: formatClock(jobElapsedSeconds(focusJob, now) ?? 0) })}`
                      : ""}
                  </span>
                  <div className="jobs-focus-metrics">
                    <span>
                      <small>{t("jobs.focus.cost")}</small>
                      <b className="mono">{focusJob.usage?.estimated_usd ? `$${focusJob.usage.estimated_usd.toFixed(4)}` : "$0.00"}</b>
                    </span>
                    <span>
                      <small>{t("jobs.focus.duration")}</small>
                      <b className="mono">{focusItem?.duration ?? "-"}</b>
                    </span>
                    <span>
                      <small>{t("jobs.focus.images")}</small>
                      <b className="mono">{focusJob.usage?.image_count ?? 0}</b>
                    </span>
                    <span>
                      <small>{t("jobs.focus.input")}</small>
                      <b className="mono">{(focusJob.usage?.input_tokens ?? 0).toLocaleString()}</b>
                    </span>
                  </div>
                </div>
              </article>
            ) : (
              <div className="jobs-focus-empty">{t("jobs.focus.empty")}</div>
            )}
            <div className="jobs-focus-note">
              <span className="jobs-cost-pill-dot" />
              {t("jobs.localProcessing")}
            </div>
          </aside>

          <section className="jobs-center-main">
          {paused ? (
            <div className="jobs-paused-note">
              <Pause size={13} />
              <span>{t("jobs.pausedNote")}</span>
            </div>
          ) : null}

          {showFilterControls ? (
            <>
              <div className="jobs-filters jobs-center-filters" role="tablist" aria-label={t("jobs.filter.aria")}>
                {filters.map((f) => (
                  <button
                    key={f.id}
                    type="button"
                    className={filter === f.id ? "jobs-filter on" : "jobs-filter"}
                    role="tab"
                    aria-selected={filter === f.id}
                    onClick={() => onFilterChange(f.id)}
                  >
                    {f.label} <span className="jobs-filter-n">{f.n}</span>
                  </button>
                ))}
                <span className="jobs-cost-pill">
                  <span className="jobs-cost-pill-dot" />
                  {t("jobs.localProcessing")}
                </span>
                {onCancelQueuedJobs && controlsEnabled && filter === "running" && queuedCount > 0 ? (
                  <button type="button" className="btn btn-secondary sm jobs-clear-queued" onClick={onCancelQueuedJobs}>
                    <Trash2 size={13} />
                    <span>{t("jobs.clearQueued")}</span>
                  </button>
                ) : null}
              </div>

              <div className="jobs-center-scroll">
                {loading ? (
                  <EmptyState title={t("jobs.loadingTitle")} body={t("jobs.loadingBody")} />
                ) : sortedJobs.length > 0 || visibleSyncingSources.length > 0 ? (
                  <div className="jobs-list-v2">
                    {visibleSyncingSources.map(renderSyncingSourceCard)}
                    {sortedJobs.map(renderCard)}
                  </div>
                ) : (
                  <EmptyState title={emptyTitle} body={emptyBody} />
                )}
              </div>
            </>
          ) : (
            <div className="jobs-center-scroll">
              <EmptyState title={t("jobs.noneTitle")} body={t("jobs.emptyBody")} />
            </div>
          )}
          </section>
        </div>
      </aside>
    </div>
  );
}
