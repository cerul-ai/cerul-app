import { useEffect, useMemo, useRef, useState } from "react";
import { Check, ChevronLeft, ChevronRight, Copy, FileVideo, Pause, Play, Search, Trash2, X } from "lucide-react";
import * as api from "../lib/api";
import { writeClipboardText } from "../lib/clipboard";
import { useT } from "../lib/i18n";
import { isActiveJob } from "../lib/items";
import {
  jobBadgeStatus,
  jobDisplayStatus,
  jobItemTitle,
  jobStageMessage,
  jobStepProgressPercent,
  jobTypeLabel,
} from "../lib/jobs";
import type { Item, Source } from "../lib/types";
import { useDialogFocus, useEscapeToClose } from "../lib/use-dismissable";
import { EmptyState } from "../components/leaf";

type JobGroup = "running" | "done" | "failed";
export type JobsFilter = "all" | JobGroup;
type RepairPhase = "idle" | "repairing" | "resolved" | "returning" | "error";

const PAGE_SIZE = 50;

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
  paused = false,
  controlsEnabled = true,
  onTogglePause,
  onFilterChange,
  onCancelJob,
  onCancelJobs,
  onCancelQueuedJobs,
  onRetryJob,
  onClose,
  onOpenSettingsFix,
  onOpenSources,
  embedded = false,
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
  onCancelJobs?: (jobs: api.JobRecord[]) => void;
  onCancelQueuedJobs?: () => void;
  onRetryJob: (job: api.JobRecord) => Promise<void>;
  onClose: () => void;
  onOpenSettingsFix: (section: string) => void;
  onOpenSources?: () => void;
  embedded?: boolean;
}) {
  const t = useT();
  const dialogRef = useRef<HTMLElement | null>(null);
  const queryInputRef = useRef<HTMLInputElement | null>(null);
  const workspaceRef = useRef<HTMLDivElement | null>(null);
  const cabinCardRef = useRef<HTMLElement | null>(null);
  const returnRowRef = useRef<HTMLDivElement | null>(null);
  const rowRefs = useRef(new Map<string, HTMLDivElement>());
  useEscapeToClose(onClose, true);
  useDialogFocus(dialogRef, !embedded);

  const [query, setQuery] = useState("");
  const [page, setPage] = useState(0);
  const [selectedIds, setSelectedIds] = useState<Set<string>>(() => new Set());
  const [selectedJobId, setSelectedJobId] = useState<string | null>(null);
  const [activeIssueId, setActiveIssueId] = useState<string | null>(null);
  const [issueOpen, setIssueOpen] = useState(false);
  const [dismissedIssueIds, setDismissedIssueIds] = useState<Set<string>>(() => new Set());
  const [repairingJob, setRepairingJob] = useState<api.JobRecord | null>(null);
  const [repairPhase, setRepairPhase] = useState<RepairPhase>("idle");
  const [repairStep, setRepairStep] = useState(0);
  const [repairError, setRepairError] = useState<string | null>(null);
  const [returningJob, setReturningJob] = useState<api.JobRecord | null>(null);
  const [exported, setExported] = useState(false);

  const sortedJobs = useMemo(
    () => [...jobs].sort((left, right) => {
      const activeDelta = Number(isActiveJob(right)) - Number(isActiveJob(left));
      return activeDelta || jobSortTime(right) - jobSortTime(left);
    }),
    [jobs],
  );
  const itemForJob = (job: api.JobRecord) =>
    job.item_id ? items.find((item) => item.id === job.item_id) ?? null : null;
  const failedJobs = sortedJobs.filter((job) => jobGroup(job) === "failed");
  const queuedJobs = sortedJobs.filter((job) => job.status === "queued");
  const runningJobs = sortedJobs.filter((job) => job.status === "running");
  const doneJobs = sortedJobs.filter((job) => jobGroup(job) === "done");
  const queuedCount = summary?.queued_jobs ?? queuedJobs.length;
  const runningCount = summary?.running_jobs ?? runningJobs.length;
  const failedCount = summary?.failed_jobs ?? failedJobs.length;
  const doneCount = summary ? summary.completed_jobs + summary.cancelled_jobs : doneJobs.length;
  const totalCount = (summary?.total_jobs ?? sortedJobs.length) + syncingSources.length;
  const activeIssueJob = sortedJobs.find((job) => job.id === activeIssueId) ?? null;
  const repairJob = repairingJob ?? activeIssueJob;
  const repairJobCanRetry = Boolean(repairJob?.item_id);
  const inspectedJob = sortedJobs.find((job) => job.id === selectedJobId) ?? sortedJobs[0] ?? null;

  // Stage-change ledger backing the inspector's activity feed. JobRecord only
  // carries the *current* stage, so transitions are recorded app-side as polls
  // land. Keyed dedupe makes the render-time mutation idempotent (safe under
  // StrictMode double-render); jobs seen mid-flight seed from started_at.
  const activityLog = useRef(new Map<string, {
    key: string;
    events: Array<{ label: string; status: string; progress: number; at: number | null }>;
  }>());
  for (const job of jobs) {
    const key = `${job.status}:${job.stage ?? ""}:${job.error ? "error" : ""}`;
    const entry = activityLog.current.get(job.id);
    if (!entry) {
      activityLog.current.set(job.id, {
        key,
        events: isActiveJob(job)
          ? [{
              label: jobStageMessage(job, t) ?? jobDisplayStatus(job, t),
              status: jobDisplayStatus(job, t),
              progress: jobStepProgressPercent(job),
              at: job.started_at !== null ? job.started_at * 1000 : null,
            }]
          : [],
      });
    } else if (entry.key !== key) {
      entry.key = key;
      entry.events.unshift({
        label: jobStageMessage(job, t) ?? jobDisplayStatus(job, t),
        status: jobDisplayStatus(job, t),
        progress: jobStepProgressPercent(job),
        at: Date.now(),
      });
      if (entry.events.length > 6) entry.events.length = 6;
    }
  }
  const inspectedActivity = inspectedJob ? activityLog.current.get(inspectedJob.id)?.events ?? [] : [];
  const activityTime = (at: number | null) =>
    at === null ? "—" : new Date(at).toTimeString().slice(0, 5);
  const inspectedItem = inspectedJob ? itemForJob(inspectedJob) : null;

  const normalizedQuery = query.trim().toLocaleLowerCase();
  const filteredSyncingSources = syncingSources.filter((source) => {
    if (filter !== "all" && filter !== "running") return false;
    if (!normalizedQuery) return true;
    return [
      source.name,
      t("jobs.type.source_discovery"),
      t("jobs.sourceDiscovery.body"),
      t("jobs.status.discovering"),
    ].some((value) => value.toLocaleLowerCase().includes(normalizedQuery));
  });
  const filteredJobs = sortedJobs.filter((job) => {
    if (filter !== "all" && jobGroup(job) !== filter) return false;
    if (!normalizedQuery) return true;
    const item = itemForJob(job);
    return [
      jobItemTitle(job, items, t),
      jobTypeLabel(job.job_type, t),
      jobStageMessage(job, t),
      jobDisplayStatus(job, t),
      item?.source ?? "",
      job.error_info?.message ?? "",
    ].some((value) => (value ?? "").toLocaleLowerCase().includes(normalizedQuery));
  });
  const ledgerJobs = filteredJobs.filter((job) => {
    if (issueOpen && repairJob?.id === job.id) return false;
    if (returningJob?.item_id && job.item_id === returningJob.item_id) return false;
    return true;
  });
  const pageCount = Math.max(1, Math.ceil(ledgerJobs.length / PAGE_SIZE));
  const safePage = Math.min(page, pageCount - 1);
  const pageJobs = ledgerJobs.slice(safePage * PAGE_SIZE, (safePage + 1) * PAGE_SIZE);
  const selectedJobs = pageJobs.filter((job) => selectedIds.has(job.id) && isActiveJob(job));
  const selectablePageJobs = pageJobs.filter(isActiveJob);
  const allPageSelected = selectablePageJobs.length > 0 && selectablePageJobs.every((job) => selectedIds.has(job.id));
  const forceMotion = import.meta.env.DEV && new URLSearchParams(window.location.hash.split("?")[1] ?? "").get("forceMotion") === "1";
  const suppressIssueFixture = import.meta.env.DEV && new URLSearchParams(window.location.hash.split("?")[1] ?? "").get("suppressIssues") === "1";
  const reduceMotion = !forceMotion && window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  const wait = (ms: number) => new Promise<void>((resolve) => window.setTimeout(resolve, reduceMotion ? 0 : ms));

  useEffect(() => {
    setPage(0);
  }, [filter, normalizedQuery]);

  useEffect(() => {
    const focusSearch = () => {
      queryInputRef.current?.focus();
      queryInputRef.current?.select();
    };
    window.addEventListener("cerul:focus-jobs-search", focusSearch);
    return () => window.removeEventListener("cerul:focus-jobs-search", focusSearch);
  }, []);

  useEffect(() => {
    if (!embedded) return;
    function handlePageKeyDown(event: globalThis.KeyboardEvent) {
      const target = event.target;
      if (
        event.metaKey || event.ctrlKey || event.altKey ||
        (target instanceof HTMLElement && (
          target.isContentEditable || target.matches("input, textarea, select")
        )) ||
        document.querySelector(".scrim, [role='dialog']")
      ) {
        return;
      }
      if ((event.key === "ArrowDown" || event.key === "ArrowUp") && pageJobs.length > 0) {
        event.preventDefault();
        const selectedIndex = Math.max(0, pageJobs.findIndex((job) => job.id === inspectedJob?.id));
        const delta = event.key === "ArrowDown" ? 1 : -1;
        const nextIndex = (selectedIndex + delta + pageJobs.length) % pageJobs.length;
        const nextJob = pageJobs[nextIndex];
        setSelectedJobId(nextJob.id);
        window.requestAnimationFrame(() => {
          document.querySelector<HTMLElement>(`[data-job-id="${CSS.escape(nextJob.id)}"] .jobs-ledger-title-button`)?.focus();
        });
        return;
      }
      if (event.key === " " && inspectedJob) {
        event.preventDefault();
        setSelectedJobId(inspectedJob.id);
        document.querySelector<HTMLElement>(`[data-job-id="${CSS.escape(inspectedJob.id)}"] .jobs-ledger-title-button`)?.focus();
      }
    }
    window.addEventListener("keydown", handlePageKeyDown);
    return () => window.removeEventListener("keydown", handlePageKeyDown);
  }, [embedded, inspectedJob?.id, pageJobs.map((job) => job.id).join("|")]);

  useEffect(() => {
    const ids = new Set(jobs.filter(isActiveJob).map((job) => job.id));
    setSelectedIds((current) => new Set([...current].filter((id) => ids.has(id))));
  }, [jobs]);

  useEffect(() => {
    if (!issueOpen || repairPhase === "repairing" || repairPhase === "resolved" || repairPhase === "returning") return;
    if (activeIssueJob && jobGroup(activeIssueJob) === "failed") return;
    setIssueOpen(false);
    setActiveIssueId(null);
    setRepairingJob(null);
    setRepairPhase("idle");
    setRepairStep(0);
    setRepairError(null);
  }, [issueOpen, repairPhase, activeIssueJob?.id, activeIssueJob?.status]);

  useEffect(() => {
    if (suppressIssueFixture || issueOpen || repairPhase !== "idle") return;
    const nextIssue = filteredJobs.find(
      (job) => jobGroup(job) === "failed" && !dismissedIssueIds.has(job.id),
    );
    if (!nextIssue) return;
    const timer = window.setTimeout(() => openIssue(nextIssue), reduceMotion ? 0 : 360);
    return () => window.clearTimeout(timer);
  }, [filteredJobs.map((job) => job.id).join("|"), issueOpen, repairPhase, dismissedIssueIds, suppressIssueFixture]);

  function flyTransfer(from: DOMRect, to: HTMLElement, job: api.JobRecord) {
    if (reduceMotion || !workspaceRef.current) return Promise.resolve();
    const workspace = workspaceRef.current;
    const bounds = workspace.getBoundingClientRect();
    const destination = to.getBoundingClientRect();
    const ghost = document.createElement("div");
    ghost.className = "jobs-transfer-ghost";
    const title = document.createElement("strong");
    title.textContent = jobItemTitle(job, items, t);
    const meta = document.createElement("span");
    meta.textContent = `${jobTypeLabel(job.job_type, t)} · ${jobDisplayStatus(job, t)}`;
    ghost.append(title, meta);
    Object.assign(ghost.style, {
      left: `${from.left - bounds.left}px`,
      top: `${from.top - bounds.top}px`,
      width: `${from.width}px`,
      height: `${from.height}px`,
    });
    workspace.appendChild(ghost);
    requestAnimationFrame(() => requestAnimationFrame(() => {
      ghost.style.transform = `translate(${destination.left - from.left}px, ${destination.top - from.top}px) scale(${destination.width / from.width}, ${destination.height / from.height})`;
    }));
    return new Promise<void>((resolve) => {
      window.setTimeout(() => {
        ghost.style.opacity = "0";
        window.setTimeout(() => { ghost.remove(); resolve(); }, 130);
      }, 430);
    });
  }

  function openIssue(job: api.JobRecord) {
    if (repairPhase !== "idle") return;
    const from = rowRefs.current.get(job.id)?.getBoundingClientRect() ?? null;
    setActiveIssueId(job.id);
    setIssueOpen(true);
    setRepairError(null);
    if (!from) return;
    requestAnimationFrame(() => requestAnimationFrame(() => {
      if (cabinCardRef.current) void flyTransfer(from, cabinCardRef.current, job);
    }));
  }

  function dismissIssue(job: api.JobRecord) {
    setDismissedIssueIds((current) => new Set(current).add(job.id));
    setIssueOpen(false);
    setActiveIssueId(null);
    setRepairingJob(null);
    setRepairPhase("idle");
    setRepairStep(0);
    setRepairError(null);
  }

  async function retryIssue(job: api.JobRecord) {
    setRepairingJob(job);
    setRepairPhase("repairing");
    setRepairStep(1);
    setRepairError(null);
    try {
      await wait(420);
      setRepairStep(2);
      await onRetryJob(job);
      await wait(420);
      setRepairStep(3);
      await wait(420);
      setRepairPhase("resolved");
      setRepairStep(4);
      await wait(720);
      setReturningJob(job);
      setRepairPhase("returning");
      await wait(0);
      const from = cabinCardRef.current?.getBoundingClientRect() ?? null;
      if (from && returnRowRef.current) await flyTransfer(from, returnRowRef.current, job);
      setDismissedIssueIds((current) => new Set(current).add(job.id));
      setIssueOpen(false);
      setActiveIssueId(null);
      setRepairingJob(null);
      setRepairPhase("idle");
      setRepairStep(0);
      await wait(1600);
      setReturningJob(null);
    } catch (error) {
      setRepairPhase("error");
      setRepairError(error instanceof Error ? error.message : String(error));
    }
  }

  function togglePageSelection() {
    setSelectedIds((current) => {
      const next = new Set(current);
      for (const job of selectablePageJobs) {
        if (allPageSelected) next.delete(job.id);
        else next.add(job.id);
      }
      return next;
    });
  }

  async function exportRecords() {
    await writeClipboardText(JSON.stringify(filteredJobs, null, 2));
    setExported(true);
    window.setTimeout(() => setExported(false), 1600);
  }

  const filters: { id: JobsFilter; label: string; n: number }[] = [
    { id: "all", label: t("jobs.filter.all"), n: totalCount },
    { id: "running", label: t(paused ? "jobs.groupQueued" : "jobs.groupRunning"), n: queuedCount + runningCount + syncingSources.length },
    { id: "done", label: t("jobs.status.completed"), n: doneCount },
    { id: "failed", label: t("jobs.status.failed"), n: failedCount },
  ];

  function ledgerRow(job: api.JobRecord, extraClass = "") {
    const item = itemForJob(job);
    const group = jobGroup(job);
    const canCancel = controlsEnabled && isActiveJob(job) && Boolean(onCancelJob);
    const selected = selectedIds.has(job.id);
    return (
      <div
        className={`jobs-ledger-row${inspectedJob?.id === job.id ? " is-inspected" : ""}${extraClass ? ` ${extraClass}` : ""}`}
        data-tone={jobBadgeStatus(job.status)}
        data-job-id={job.id}
        aria-selected={inspectedJob?.id === job.id}
        key={`${extraClass}:${job.id}`}
        ref={(node) => {
          if (extraClass.includes("jobs-return-row")) returnRowRef.current = node;
          else if (node) rowRefs.current.set(job.id, node);
          else rowRefs.current.delete(job.id);
        }}
      >
        <span className="jobs-ledger-check">
          <input
            type="checkbox"
            checked={selected}
            disabled={!isActiveJob(job)}
            aria-label={jobItemTitle(job, items, t)}
            onChange={(event) => setSelectedIds((current) => {
              const next = new Set(current);
              if (event.currentTarget.checked) next.add(job.id); else next.delete(job.id);
              return next;
            })}
          />
        </span>
        <button type="button" className="jobs-ledger-title-button" onClick={() => setSelectedJobId(job.id)}>
          <span className={item?.thumbnailUrl ? "jobs-ledger-thumb has-image" : "jobs-ledger-thumb"}>
            {item?.thumbnailUrl ? <img src={item.thumbnailUrl} alt="" /> : <FileVideo size={15} />}
          </span>
          <span className="clamp2">{jobItemTitle(job, items, t)}</span>
        </button>
        <span className="muted clamp1">{jobTypeLabel(job.job_type, t)}</span>
        <span className="muted clamp1">{item?.source ?? t("jobs.localProcessing")}</span>
        <span className="jobs-ledger-stage clamp1">
          {jobStageMessage(job, t)}
          {job.status === "running" ? <i><b style={{ width: `${jobStepProgressPercent(job)}%` }} /></i> : null}
        </span>
        <span className="job-status-pill" data-tone={jobBadgeStatus(job.status)}>{jobDisplayStatus(job, t)}</span>
        <span className="jobs-ledger-action">
          {group === "failed" ? <button type="button" onClick={() => openIssue(job)}>{t("jobs.repair.open")}</button> : null}
          {canCancel ? <button type="button" onClick={() => onCancelJob?.(job)}>{t("jobs.cancelAria")}</button> : null}
        </span>
      </div>
    );
  }

  const ledger = (
      <section
        ref={dialogRef}
        className={embedded ? "jobs-ledger-dialog jobs-sheet is-page" : "jobs-ledger-dialog jobs-sheet"}
        role={embedded ? "region" : "dialog"}
        aria-modal={embedded ? undefined : "true"}
        aria-labelledby="jobs-ledger-title"
        onMouseDown={(event) => {
          if (!embedded) event.stopPropagation();
        }}
      >
        <header className="jobs-ledger-head">
          <div className="jobs-ledger-title-group">
            {embedded ? (
              <button type="button" className="jobs-ledger-back" onClick={onClose}>
                <ChevronLeft size={16} />{t("jobs.back")}
              </button>
            ) : null}
            <div>
              <p className="section-label eyebrow">{t("jobs.eyebrow")}</p>
              <h2 id="jobs-ledger-title">{t("jobs.ledger.title")}</h2>
              <p>{t("jobs.ledger.subtitle", { count: totalCount })}</p>
            </div>
          </div>
          <div className="jobs-ledger-head-actions">
            <button type="button" className="btn btn-secondary sm" onClick={() => void exportRecords()}>
              {exported ? <Check size={13} /> : <Copy size={13} />}{exported ? t("jobs.ledger.exported") : t("jobs.ledger.export")}
            </button>
            {onTogglePause && controlsEnabled ? (
              <button type="button" className="btn btn-primary sm" onClick={onTogglePause}>
                {paused ? <Play size={13} /> : <Pause size={13} />}{paused ? t("jobs.resume") : t("jobs.pause")}
              </button>
            ) : null}
            {!embedded ? <button type="button" className="btn-icon" aria-label={t("jobs.closeAria")} onClick={onClose}><X size={17} /></button> : null}
          </div>
        </header>

        <div className="jobs-ledger-toolbar">
          <label><Search size={15} /><input ref={queryInputRef} value={query} onChange={(event) => setQuery(event.currentTarget.value)} placeholder={t("jobs.ledger.search")} /></label>
          <div role="tablist" aria-label={t("jobs.filter.aria")}>
            {filters.map((entry) => (
              <button key={entry.id} type="button" role="tab" aria-selected={filter === entry.id} className={filter === entry.id ? "active" : ""} onClick={() => onFilterChange(entry.id)}>
                {entry.label} <span>{entry.n}</span>
              </button>
            ))}
          </div>
          <span className="jobs-ledger-local"><i />{t("jobs.localProcessing")}</span>
          {onCancelQueuedJobs && controlsEnabled && queuedCount > 0 && (filter === "all" || filter === "running") ? <button type="button" className="btn btn-secondary sm" onClick={onCancelQueuedJobs}>{t("jobs.clearQueued")}</button> : null}
        </div>

        <div ref={workspaceRef} className={issueOpen && repairJob ? "jobs-ledger-workspace has-issue" : "jobs-ledger-workspace"}>
          <div className="jobs-repair-clip">
            {repairJob ? (
              <aside className={`jobs-repair-cabin phase-${repairPhase}`} aria-label={t("jobs.repair.title")}>
                <header><strong>{t("jobs.repair.title")}</strong><span>{t("jobs.repair.count", { count: failedCount })}</span></header>
                <article className="jobs-repair-card" ref={cabinCardRef}>
                  <strong>{jobItemTitle(repairJob, items, t)}</strong>
                  <span>{jobTypeLabel(repairJob.job_type, t)} · {itemForJob(repairJob)?.source ?? t("jobs.localProcessing")}</span>
                </article>
                <div className="jobs-repair-detail">
                  <h3>{repairJob.error_info ? t(`jobs.error.${repairJob.error_info.code}`, { capability: jobTypeLabel(repairJob.job_type, t) }) : jobDisplayStatus(repairJob, t)}</h3>
                  <p>{repairJob.error_info?.message || repairJob.error || t("jobs.repair.body")}</p>
                  {repairPhase === "idle" || repairPhase === "error" ? (
                    <div className="jobs-repair-actions">
                      {repairJobCanRetry ? <button type="button" className="btn btn-primary sm" disabled={!controlsEnabled} onClick={() => void retryIssue(repairJob)}>{t("jobs.repair.retry")}</button> : null}
                      {repairJob.error_info?.code === "source_unavailable" && onOpenSources ? <button type="button" className="btn btn-secondary sm" onClick={onOpenSources}>{t("jobs.viewSources")}</button> : null}
                      {repairJob.error_info?.code !== "source_unavailable" && repairJob.error_info?.settings_section ? <button type="button" className="btn btn-secondary sm" onClick={() => onOpenSettingsFix(repairJob.error_info?.settings_section ?? "General")}>{t("jobs.fixSettings")}</button> : null}
                      <button type="button" className="btn btn-ghost sm" onClick={() => dismissIssue(repairJob)}>{t("jobs.repair.later")}</button>
                    </div>
                  ) : null}
                  {repairPhase === "repairing" || repairPhase === "resolved" || repairPhase === "returning" ? (
                    <div className="jobs-repair-steps">
                      {[t("jobs.repair.step.connect"), t("jobs.repair.step.verify"), t("jobs.repair.step.requeue")].map((label, index) => {
                        const number = index + 1;
                        const done = repairStep > number;
                        return <span key={label} className={done ? "done" : repairStep === number ? "active" : ""}><i>{done ? "✓" : number}</i>{label}</span>;
                      })}
                    </div>
                  ) : null}
                  {repairPhase === "resolved" || repairPhase === "returning" ? <div className="jobs-repair-success">{t("jobs.repair.success")}</div> : null}
                  {repairPhase === "error" && repairError ? <div className="jobs-repair-error" role="alert">{t("jobs.repair.failed", { error: repairError })}</div> : null}
                </div>
              </aside>
            ) : null}
          </div>

          <section className="jobs-ledger-main">
            {paused ? <div className="jobs-paused-note"><Pause size={13} /><span>{t("jobs.pausedNote")}</span></div> : null}
            <div className="jobs-ledger-table">
              <div className="jobs-ledger-columns" aria-hidden="true">
                <span><input type="checkbox" checked={allPageSelected} readOnly /></span><span>{t("jobs.ledger.col.content")}</span><span>{t("jobs.ledger.col.type")}</span><span>{t("jobs.ledger.col.source")}</span><span>{t("jobs.ledger.col.stage")}</span><span>{t("jobs.ledger.col.status")}</span><span>{t("jobs.ledger.col.action")}</span>
              </div>
              {filteredSyncingSources.map((source) => (
                <div className="jobs-ledger-row jobs-source-discovery-row" key={`source:${source.id}`}>
                  <span /><strong className="clamp1">{source.name}</strong><span className="muted">{t("jobs.type.source_discovery")}</span><span className="muted">{source.name}</span><span className="jobs-ledger-stage">{t("jobs.sourceDiscovery.body")}</span><span className="job-status-pill" data-tone="running">{t("jobs.status.discovering")}</span><span />
                </div>
              ))}
              {returningJob ? ledgerRow(returningJob, "jobs-return-row") : null}
              {loading ? <EmptyState title={t("jobs.loadingTitle")} body={t("jobs.loadingBody")} /> : pageJobs.length > 0 ? pageJobs.map((job) => ledgerRow(job)) : filteredSyncingSources.length === 0 ? <EmptyState title={t("jobs.noneTitle")} body={t("jobs.emptyBody")} /> : null}
            </div>
            <footer className="jobs-ledger-footer">
              <label><input type="checkbox" checked={allPageSelected} onChange={togglePageSelection} />{t("jobs.ledger.selectPage")}</label>
              <span>{t("jobs.ledger.selected", { count: selectedJobs.length })}</span>
              {selectedJobs.length > 0 && onCancelJobs ? <button type="button" className="btn btn-secondary sm" onClick={() => onCancelJobs(selectedJobs)}><Trash2 size={13} />{t("jobs.ledger.cancelSelected")}</button> : null}
              <div className="jobs-ledger-pagination">
                <span>{t("jobs.ledger.page", { page: safePage + 1, pages: pageCount })}</span>
                <button type="button" disabled={safePage === 0} onClick={() => setPage((value) => Math.max(0, value - 1))}><ChevronLeft size={14} /></button>
                <button type="button" disabled={safePage >= pageCount - 1} onClick={() => setPage((value) => Math.min(pageCount - 1, value + 1))}><ChevronRight size={14} /></button>
              </div>
            </footer>
          </section>
          {!issueOpen ? (
            <aside className="jobs-current-inspector" aria-label={t("jobs.inspector.title")}>
              <header>
                <div><p className="section-label">{t("jobs.inspector.eyebrow")}</p><h3>{t("jobs.inspector.title")}</h3></div>
                {inspectedJob ? <span className="job-status-pill" data-tone={jobBadgeStatus(inspectedJob.status)}>{jobDisplayStatus(inspectedJob, t)}</span> : null}
              </header>
              {inspectedJob ? (
                <>
                  <article className="jobs-inspector-media">
                    <span className={inspectedItem?.thumbnailUrl ? "jobs-inspector-thumb has-image" : "jobs-inspector-thumb"}>
                      {inspectedItem?.thumbnailUrl ? <img src={inspectedItem.thumbnailUrl} alt="" /> : <FileVideo size={22} />}
                    </span>
                    <span><strong className="clamp2">{jobItemTitle(inspectedJob, items, t)}</strong><small>{inspectedItem?.source ?? t("jobs.localProcessing")} · {jobTypeLabel(inspectedJob.job_type, t)}</small></span>
                  </article>
                  <div className="jobs-inspector-progress">
                    <span><b>{t("jobs.inspector.progress")}</b><code>{jobStepProgressPercent(inspectedJob)}%</code></span>
                    <i><b style={{ width: `${jobStepProgressPercent(inspectedJob)}%` }} /></i>
                  </div>
                  <div className="jobs-inspector-stages">
                    {[t("jobs.stage.fetching"), t("jobs.stage.transcribing"), t("jobs.stage.embedding_frames"), t("understanding.title"), t("jobs.stage.writing_index")].map((label, index) => {
                      const progress = jobStepProgressPercent(inspectedJob);
                      const current = inspectedJob.status === "completed" ? 5 : Math.min(4, Math.floor(progress / 20));
                      const done = index < current || inspectedJob.status === "completed";
                      const active = !done && index === current && isActiveJob(inspectedJob);
                      return <span key={label} className={done ? "done" : active ? "active" : ""}><i>{done ? "✓" : index + 1}</i><b>{label}</b><code>{done ? t("jobs.status.completed") : active ? jobDisplayStatus(inspectedJob, t) : "—"}</code></span>;
                    })}
                  </div>
                  <dl className="jobs-inspector-stats">
                    <div><dt>{t("jobs.ledger.col.source")}</dt><dd>{inspectedItem?.source ?? t("jobs.localProcessing")}</dd></div>
                    <div><dt>{t("jobs.ledger.col.stage")}</dt><dd>{jobStageMessage(inspectedJob, t)}</dd></div>
                    <div><dt>{t("jobs.inspector.media")}</dt><dd>{inspectedItem?.duration || "—"}</dd></div>
                  </dl>
                  <div className="jobs-inspector-log">
                    <strong>{t("jobs.inspector.activity")}</strong>
                    {inspectedJob.error ? <p className="danger">{inspectedJob.error}</p> : null}
                    {inspectedActivity.length > 0
                      ? inspectedActivity.map((event, index) => (
                          <article className={index === 0 ? "jobs-activity-event is-current" : "jobs-activity-event"} key={`${event.label}:${event.at ?? index}`}>
                            <time>{activityTime(event.at)}</time>
                            <i aria-hidden="true" />
                            <span><b>{event.label}</b><small>{event.status}</small></span>
                            <code>{event.progress}%</code>
                          </article>
                        ))
                      : <article className="jobs-activity-event is-current"><time>—</time><i aria-hidden="true" /><span><b>{jobStageMessage(inspectedJob, t)}</b><small>{jobDisplayStatus(inspectedJob, t)}</small></span><code>{jobStepProgressPercent(inspectedJob)}%</code></article>}
                  </div>
                </>
              ) : <EmptyState title={t("jobs.focus.empty")} body={t("jobs.emptyBody")} />}
            </aside>
          ) : null}
        </div>
      </section>
  );

  if (embedded) {
    return <div className="jobs-ledger-page">{ledger}</div>;
  }

  return (
    <div className="scrim sheet-backdrop jobs-ledger-backdrop" role="presentation" onMouseDown={onClose}>
      {ledger}
    </div>
  );
}
