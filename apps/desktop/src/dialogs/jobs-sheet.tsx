import { useRef } from "react";
// Slide-out sheet listing indexing jobs. Extracted from App.tsx
// (B13 Phase D).

import { X } from "lucide-react";
import * as api from "../lib/api";
import { formatUsd } from "../lib/formatters";
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
import { ProgressBar, StatusBadge } from "../components/transcript";

export function JobsSheet({
  jobs,
  items,
  stepStarts,
  onClose,
  onOpenSettingsFix,
  onOpenSources,
}: {
  jobs: api.JobRecord[];
  items: Item[];
  stepStarts: Record<string, number>;
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
    return (b.started_at ?? b.finished_at ?? 0) - (a.started_at ?? a.finished_at ?? 0);
  });
  const activeJobs = sortedJobs.filter(isActiveJob);
  const recentJobs = sortedJobs.filter((job) => !isActiveJob(job));
  // Incurred remote cost across the running batch (QA: spend must be visible
  // before/while it happens, not only after). Local processing reports $0.
  const activeCostUsd = activeJobs.reduce(
    (sum, job) => sum + (job.usage?.estimated_usd ?? 0),
    0,
  );
  const now = useNowSeconds(activeJobs.length > 0);
  // ① The header used to read "No jobs running" even while failed jobs sat in the
  // list right below it. Summarise running + failed so the title can't contradict
  // the rows. (Batch spend stays in the cost card.)
  const failedJobs = sortedJobs.filter((job) => jobBadgeStatus(job.status) === "failed");
  const summaryParts: string[] = [];
  if (activeJobs.length > 0) {
    summaryParts.push(
      t(activeJobs.length === 1 ? "jobs.activeCountOne" : "jobs.activeCountOther", {
        count: activeJobs.length,
      }),
    );
  }
  if (failedJobs.length > 0) {
    summaryParts.push(t("jobs.failedCount", { count: failedJobs.length }));
  }
  const title = summaryParts.length > 0 ? summaryParts.join(" · ") : t("jobs.noneTitle");

  const renderRow = (job: api.JobRecord) => {
    const stage = jobStageMessage(job, t);
    const usage = jobUsageLabel(job, t);
    const badgeStatus = jobBadgeStatus(job.status);
    const isRunning = job.status === "running";
    const isFailed = badgeStatus === "failed";
    const step = jobStepInfo(job);
    const stepElapsed = jobStepElapsedSeconds(job, stepStarts, now);
    const elapsed = jobElapsedSeconds(job, now);
    const meta = [
      step ? t("jobs.step", { current: step.current, total: step.total }) : null,
      stepElapsed !== null ? t("jobs.stepElapsed", { duration: formatClock(stepElapsed) }) : null,
      elapsed !== null ? t("jobs.elapsed", { duration: formatClock(elapsed) }) : null,
      jobEtaLabel(job, now, t),
    ].filter(Boolean);
    return (
      <article className="job-row" key={job.id}>
        <span className={`job-dot ${badgeStatus}`} aria-hidden="true" />
        <div className="job-row-main">
          <div className="row gap-2">
            <strong className="clamp1 grow">{jobItemTitle(job, items, t)}</strong>
            <StatusBadge status={badgeStatus} label={jobDisplayStatus(job, t)} />
          </div>
          <span className="muted">{jobTypeLabel(job.job_type, t)}</span>
          {/* ④ A failed card no longer carries a frozen 0% progress bar — progress
              is for running jobs only; failure reads from the red dot + badge. */}
          {isRunning ? <ProgressBar value={jobStepProgressPercent(job)} animated /> : null}
          {/* ② Raw HTTP/JSON error payloads are tucked into a collapsible instead
              of dumped in the user's face. Friendly mapping is task #7. */}
          {isFailed ? (
            <>
              {job.error_info ? (
                <div className="job-fix">
                  {/* Message localized client-side by error code so it follows
                      the UI language (the API's friendly string is zh-only). */}
                  <p>
                    {t(`jobs.error.${job.error_info.code}`, {
                      capability: jobTypeLabel(job.job_type, t),
                    })}
                  </p>
                  {job.error_info.code === "source_unavailable" ? (
                    onOpenSources ? (
                      <button
                        type="button"
                        className="btn btn-primary sm"
                        onClick={onOpenSources}
                      >
                        {t("jobs.viewSources")}
                      </button>
                    ) : null
                  ) : job.error_info.code === "unknown_processing_error" ? null : (
                    <button
                      type="button"
                      className="btn btn-primary sm"
                      onClick={() => onOpenSettingsFix(job.error_info?.settings_section ?? "Models")}
                    >
                      {t("jobs.fixSettings")}
                    </button>
                  )}
                </div>
              ) : null}
              {job.error ? (
                <details className="job-tech">
                  <summary>{t("jobs.tech.summary")}</summary>
                  <pre className="job-tech-raw mono">{job.error}</pre>
                </details>
              ) : null}
            </>
          ) : stage ? (
            <p className="muted">{stage}</p>
          ) : null}
          {meta.length > 0 ? <p className="job-meta faint mono">{meta.join(" · ")}</p> : null}
          {usage ? <p className="job-usage faint mono">{usage}</p> : null}
        </div>
      </article>
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
          <div>
            <p className="section-label eyebrow">{t("jobs.eyebrow")}</p>
            <h2 id="jobs-title" className="drawer-title">
              {title}
            </h2>
          </div>
          <button
            className="btn-icon"
            type="button"
            aria-label={t("jobs.closeAria")}
            onClick={onClose}
          >
            <X size={17} />
          </button>
        </header>

        <div className="drawer-body jobs-body">
          {activeJobs.length > 0 ? (
            <section className="jobs-cost-card">
              <div className="jobs-cost-main">
                <span className="jobs-cost-label">{t("jobs.cost.title")}</span>
                <strong className="jobs-cost-value mono">{formatUsd(activeCostUsd)}</strong>
              </div>
              <p className="jobs-cost-note">
                {activeCostUsd > 0 ? t("jobs.cost.noteRemote") : t("jobs.cost.noteLocal")}
              </p>
            </section>
          ) : null}
          {sortedJobs.length > 0 ? (
            <>
              {activeJobs.length > 0 ? (
                <>
                  <p className="job-group-label">{t("jobs.groupRunning")}</p>
                  {activeJobs.map(renderRow)}
                </>
              ) : null}
              {recentJobs.length > 0 ? (
                <>
                  <p className="job-group-label">{t("jobs.groupRecent")}</p>
                  {recentJobs.map(renderRow)}
                </>
              ) : null}
            </>
          ) : (
            <EmptyState title={t("jobs.noneTitle")} body={t("jobs.emptyBody")} />
          )}
        </div>
      </aside>
    </div>
  );
}
