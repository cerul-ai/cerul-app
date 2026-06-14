// Always-visible indexing banner for the library screen. The rail only carries
// a small dot; when work is running the user otherwise has to open the Jobs
// sheet to learn anything. This summarizes the active jobs inline — count,
// current step/stage, aggregate progress, live elapsed/ETA — and opens the
// sheet on click. It also surfaces a "no update for ~N" hint so a genuinely
// wedged job is visible instead of looking like slow-but-fine progress.

import { useEffect, useRef } from "react";
import { ChevronRight, Loader2 } from "lucide-react";
import * as api from "../lib/api";
import { formatUsd } from "../lib/formatters";
import { useT } from "../lib/i18n";
import { isActiveJob } from "../lib/items";
import {
  formatClock,
  jobElapsedSeconds,
  jobEtaLabel,
  jobStageMessage,
  jobStepElapsedSeconds,
  jobStepInfo,
  jobStepProgressPercent,
} from "../lib/jobs";
import type { Item } from "../lib/types";
import { useNowSeconds } from "../lib/use-now";
import { ProgressBar } from "./transcript";

const STALL_THRESHOLD_SEC = 90;

export function IndexingStrip({
  jobs,
  items,
  stepStarts,
  onOpen,
}: {
  jobs: api.JobRecord[];
  items: Item[];
  stepStarts: Record<string, number>;
  onOpen: () => void;
}) {
  const t = useT();
  const active = jobs.filter(isActiveJob);
  const now = useNowSeconds(active.length > 0);

  // Representative job = the running job furthest along (the "current" work).
  const running = active.filter((job) => job.status === "running");
  const rep = running.slice().sort((a, b) => b.progress - a.progress)[0] ?? null;
  const repPct = rep ? Math.round(rep.progress * 100) : -1;

  // Stopwatch for "last progress change": reset only when the watched job or its
  // percent actually changes. `now` ticking re-renders each second so the
  // elapsed-since-change grows visibly between polls.
  const changeRef = useRef<{ key: string; at: number }>({ key: "", at: now });
  useEffect(() => {
    changeRef.current = { key: `${rep?.id ?? ""}:${repPct}`, at: Date.now() / 1000 };
  }, [rep?.id, repPct]);

  if (active.length === 0) {
    return null;
  }

  const queued = active.length - running.length;
  // Incurred remote spend across the active batch — $0.00 while everything is
  // on-device (handoff: the cost reads green right in the banner, not only in
  // the Tasks drawer).
  const activeCostUsd = active.reduce(
    (sum, job) => sum + (job.usage?.estimated_usd ?? 0),
    0,
  );
  const costScope = activeCostUsd > 0 ? t("indexing.strip.remoteCost") : t("indexing.strip.localCost");
  // Step-based aggregate: queued jobs count as 0 so the bar represents the whole
  // active batch, not only the currently running subset.
  const avgStepPercent =
    active.length > 0
      ? active.reduce(
          (sum, job) => sum + (job.status === "running" ? jobStepProgressPercent(job) : 0),
          0,
        ) / active.length
      : 0;
  const repItem = rep ? items.find((item) => item.id === rep.item_id) : undefined;
  const repTitle = repItem?.title ?? rep?.item_id ?? null;
  const stageMessage = rep ? jobStageMessage(rep, t) : null;
  const step = rep ? jobStepInfo(rep) : null;
  const stepElapsed = rep ? jobStepElapsedSeconds(rep, stepStarts, now) : null;
  const elapsed = rep ? jobElapsedSeconds(rep, now) : null;
  const eta = rep ? jobEtaLabel(rep, now, t) : null;
  const stalledFor = rep ? now - changeRef.current.at : 0;
  const stalled = rep !== null && rep.job_type !== "index_audio" && stalledFor > STALL_THRESHOLD_SEC;

  const title = t(active.length === 1 ? "indexing.strip.one" : "indexing.strip.other", {
    count: active.length,
  });

  return (
    <button
      type="button"
      className="indexing-strip"
      onClick={onOpen}
      aria-label={t("indexing.strip.openAria")}
    >
      <Loader2 size={16} className="indexing-strip__spin" aria-hidden="true" />
      <span className="indexing-strip__body">
        <span className="indexing-strip__line">
          <strong>{title}</strong>
          {queued > 0 ? (
            <span className="muted">{t("indexing.strip.queuedSuffix", { count: queued })}</span>
          ) : null}
          {step ? (
            <span className="indexing-strip__step">
              {t("jobs.step", { current: step.current, total: step.total })}
            </span>
          ) : null}
        </span>
        {repTitle || stageMessage ? (
          <span className="indexing-strip__meta muted clamp1">
            {[repTitle, stageMessage].filter(Boolean).join(" · ")}
          </span>
        ) : null}
        <span className="indexing-strip__track">
          <ProgressBar value={Math.round(avgStepPercent)} animated={running.length > 0} />
          <span className="indexing-strip__pct mono">{Math.round(avgStepPercent)}%</span>
        </span>
      </span>
      <span className="indexing-strip__side mono">
        {stalled ? (
          <span className="indexing-strip__stalled">
            {t("indexing.strip.stalled", { duration: formatClock(stalledFor) })}
          </span>
        ) : (
          <>
            {stepElapsed !== null ? (
              <span>{t("jobs.stepElapsed", { duration: formatClock(stepElapsed) })}</span>
            ) : elapsed !== null ? (
              <span>{t("jobs.elapsed", { duration: formatClock(elapsed) })}</span>
            ) : null}
            {eta ? <span className="faint">{eta}</span> : null}
          </>
        )}
        <span className="indexing-strip__cost">{formatUsd(activeCostUsd)} · {costScope}</span>
      </span>
      <ChevronRight size={16} aria-hidden="true" />
    </button>
  );
}
