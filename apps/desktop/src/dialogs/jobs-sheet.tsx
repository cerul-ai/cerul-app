import { useRef, useState } from "react";
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
import type { Item } from "../lib/types";
import { useDialogFocus, useEscapeToClose } from "../lib/use-dismissable";
import { useNowSeconds } from "../lib/use-now";
import { EmptyState } from "../components/leaf";
import { ProgressBar } from "../components/transcript";

type JobGroup = "running" | "done" | "failed";

function jobGroup(job: api.JobRecord): JobGroup {
  const badge = jobBadgeStatus(job.status);
  if (badge === "failed") return "failed";
  if (badge === "indexed") return "done";
  return "running";
}

export function JobsSheet({
  jobs,
  items,
  stepStarts,
  paused = false,
  controlsEnabled = true,
  onTogglePause,
  onCancelJob,
  onClose,
  onOpenSettingsFix,
  onOpenSources,
}: {
  jobs: api.JobRecord[];
  items: Item[];
  stepStarts: Record<string, number>;
  paused?: boolean;
  controlsEnabled?: boolean;
  onTogglePause?: () => void;
  onCancelJob?: (job: api.JobRecord) => void;
  onClose: () => void;
  onOpenSettingsFix: (section: string) => void;
  onOpenSources?: () => void;
}) {
  const t = useT();
  useEscapeToClose(onClose);
  const dialogRef = useRef<HTMLElement | null>(null);
  useDialogFocus(dialogRef);
  const [filter, setFilter] = useState<"all" | JobGroup>("all");

  const sortedJobs = [...jobs].sort((a, b) => {
    const activeDelta = Number(isActiveJob(b)) - Number(isActiveJob(a));
    if (activeDelta !== 0) {
      return activeDelta;
    }
    return (b.started_at ?? b.finished_at ?? 0) - (a.started_at ?? a.finished_at ?? 0);
  });
  const activeJobs = sortedJobs.filter(isActiveJob);
  const queuedJobs = activeJobs.filter((job) => job.status === "queued");
  const runningJobs = activeJobs.filter((job) => job.status === "running");
  const onlyPausedQueuedJobs = paused && runningJobs.length === 0 && queuedJobs.length > 0;
  const failedJobs = sortedJobs.filter((job) => jobGroup(job) === "failed");
  const doneJobs = sortedJobs.filter((job) => jobGroup(job) === "done");
  const now = useNowSeconds(activeJobs.length > 0);

  const filters: { id: "all" | JobGroup; label: string; n: number }[] = [
    { id: "all", label: t("jobs.filter.all"), n: sortedJobs.length },
    { id: "running", label: t(paused ? "jobs.groupQueued" : "jobs.groupRunning"), n: activeJobs.length },
    { id: "done", label: t("jobs.status.completed"), n: doneJobs.length },
    { id: "failed", label: t("jobs.status.failed"), n: failedJobs.length },
  ];
  const visibleJobs = sortedJobs.filter((job) => filter === "all" || filter === jobGroup(job));

  // Header summary, with the failed count tinted red (prototype §H).
  const runningLabel =
    onlyPausedQueuedJobs
      ? t(queuedJobs.length === 1 ? "jobs.queuedCountOne" : "jobs.queuedCountOther", {
          count: queuedJobs.length,
        })
      : activeJobs.length > 0
      ? t(activeJobs.length === 1 ? "jobs.activeCountOne" : "jobs.activeCountOther", {
          count: activeJobs.length,
        })
      : null;

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
      <div className="jobs-tl-item" key={job.id}>
        <span className={`jobs-tl-node ${tone}`} aria-hidden="true" />
        <div className={`jobs-tl-card ${tone}`}>
          <div className="jobs-tl-head">
            <span className="jobs-tl-title clamp1">{jobItemTitle(job, items, t)}</span>
            <span className={`jobs-tl-pill ${tone}`}>
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

          <div className="jobs-tl-type">{typeLine}</div>

          {isRunning ? (
            <div className="jobs-tl-progress">
              <ProgressBar value={jobStepProgressPercent(job)} animated />
              <span className="jobs-tl-pct mono">{jobStepProgressPercent(job)}%</span>
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

          {!isFailed && !isRunning && stage ? <p className="muted jobs-tl-stage">{stage}</p> : null}

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
      </div>
    );
  };

  return (
    <div className="scrim sheet-backdrop" role="presentation" onMouseDown={onClose}>
      <aside
        ref={dialogRef}
        className="drawer jobs-sheet"
        role="dialog"
        aria-modal="true"
        aria-labelledby="jobs-title"
        onMouseDown={(event) => event.stopPropagation()}
      >
        <header className="drawer-head dialog-header">
          <div className="grow">
            <p className="section-label eyebrow">{t("jobs.eyebrow")}</p>
            <h2 id="jobs-title" className="drawer-title">
              {runningLabel ? <span>{runningLabel}</span> : null}
              {runningLabel && failedJobs.length > 0 ? <span> · </span> : null}
              {failedJobs.length > 0 ? (
                <span className="jobs-title-failed">{t("jobs.failedCount", { count: failedJobs.length })}</span>
              ) : null}
              {!runningLabel && failedJobs.length === 0 ? <span>{t("jobs.noneTitle")}</span> : null}
            </h2>
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

        <div className="drawer-body jobs-body">
          {paused ? (
            <div className="jobs-paused-note">
              <Pause size={13} />
              <span>{t("jobs.pausedNote")}</span>
            </div>
          ) : null}

          {sortedJobs.length > 0 ? (
            <>
              <div className="jobs-filters">
                {filters.map((f) => (
                  <button
                    key={f.id}
                    type="button"
                    className={filter === f.id ? "jobs-filter on" : "jobs-filter"}
                    onClick={() => setFilter(f.id)}
                  >
                    {f.label} <span className="jobs-filter-n">{f.n}</span>
                  </button>
                ))}
                <span className="jobs-cost-pill">
                  <span className="jobs-cost-pill-dot" />
                  {t("jobs.localProcessing")}
                </span>
              </div>

              {visibleJobs.length > 0 ? (
                <div className="jobs-timeline">
                  <span className="jobs-timeline-line" aria-hidden="true" />
                  {visibleJobs.map(renderCard)}
                </div>
              ) : (
                <EmptyState title={t("jobs.noneTitle")} body={t("jobs.emptyBody")} />
              )}
            </>
          ) : (
            <EmptyState title={t("jobs.noneTitle")} body={t("jobs.emptyBody")} />
          )}
        </div>
      </aside>
    </div>
  );
}
