// Slide-out sheet listing indexing jobs. Extracted from App.tsx
// (B13 Phase D).

import { X } from "lucide-react";
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
import { useNowSeconds } from "../lib/use-now";
import { EmptyState } from "../components/leaf";
import { ProgressBar, StatusBadge } from "../components/transcript";

export function JobsSheet({
  jobs,
  items,
  stepStarts,
  onClose,
}: {
  jobs: api.JobRecord[];
  items: Item[];
  stepStarts: Record<string, number>;
  onClose: () => void;
}) {
  const t = useT();
  const sortedJobs = [...jobs].sort((a, b) => {
    const activeDelta = Number(isActiveJob(b)) - Number(isActiveJob(a));
    if (activeDelta !== 0) {
      return activeDelta;
    }
    return (b.started_at ?? b.finished_at ?? 0) - (a.started_at ?? a.finished_at ?? 0);
  });
  const activeJobs = sortedJobs.filter(isActiveJob);
  const recentJobs = sortedJobs.filter((job) => !isActiveJob(job));
  const now = useNowSeconds(activeJobs.length > 0);
  const title =
    activeJobs.length > 0
      ? t(
          activeJobs.length === 1 ? "jobs.activeCountOne" : "jobs.activeCountOther",
          { count: activeJobs.length },
        )
      : t("jobs.noneTitle");

  const renderRow = (job: api.JobRecord) => {
    const stage = jobStageMessage(job, t);
    const usage = jobUsageLabel(job, t);
    const badgeStatus = jobBadgeStatus(job.status);
    const isRunning = job.status === "running";
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
          <ProgressBar value={jobStepProgressPercent(job)} animated={isRunning} />
          {stage ? <p className="muted">{stage}</p> : null}
          {meta.length > 0 ? <p className="job-meta faint mono">{meta.join(" · ")}</p> : null}
          {usage ? <p className="job-usage faint mono">{usage}</p> : null}
        </div>
      </article>
    );
  };

  return (
    <div className="scrim sheet-backdrop" role="presentation" onMouseDown={onClose}>
      <aside
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
