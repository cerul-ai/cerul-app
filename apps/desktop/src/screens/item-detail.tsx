import {
  Check,
  ChevronRight,
  Copy,
  Download,
  ExternalLink,
  Folder,
  Loader2,
  MoreHorizontal,
  Play,
  RefreshCcw,
  Sparkles,
  Star,
  Trash2,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { RefObject } from "react";
import {
  ClipExportButton,
  resolveClipTarget as resolveClipTarget_,
  type ClipTarget,
} from "../components/clip-export-popover";
import { DetailIssuePanel } from "../components/detail-issue-panel";
import { FrameStrip } from "../components/FrameStrip";
import { CerulPlayer, type PlayerChapter, type PlayerMarker } from "../components/player";
import { SplitStage } from "../components/SplitStage";
import { SummaryCard } from "../components/SummaryCard";
import { InlineNotice } from "../components/leaf";
import { TranscriptList, TranscriptSkeleton } from "../components/transcript";
import * as api from "../lib/api";
import { writeClipboardText } from "../lib/clipboard";
import { canOpenOriginalSource, timestampDeepLink } from "../lib/detail";
import { openDialog, invokeHostCommand } from "../lib/desktopHost";
import {
  basenameFromPath,
  buildMomentCitation,
  errorMessage,
  extractChunkIdFromThumbnail,
  formatTimestamp,
  parseTimestampSeconds,
} from "../lib/formatters";
import { useT, type TFunction } from "../lib/i18n";
import {
  isNearEndPosition,
  itemDetailIssue,
} from "../lib/items";
import { forgetLastOpened, recordLastOpened } from "../lib/last-opened";
import { mapChunkRecords, selectPlaybackChunkId } from "../lib/results";
import { useClickOutside, useEscapeToClose } from "../lib/use-dismissable";
import { itemModalityLabel } from "../components/cards";
import type { ApiStatus, Item, RequestConfirm, TranscriptLine } from "../lib/types";

const transcript: TranscriptLine[] = [];

function hasOpenModalSurface() {
  return Boolean(
    document.querySelector(".scrim, .account-pop, .menu, .model-combobox__pop, [role='dialog']"),
  );
}

function syncVideoToTimestamp(
  video: HTMLVideoElement,
  timestamp: string,
  opts: {
    shouldPlay: boolean;
    onPlayBlocked?: () => void;
  },
) {
  const targetSeconds = parseTimestampSeconds(timestamp);
  if (!Number.isFinite(targetSeconds)) {
    return () => undefined;
  }

  let cancelled = false;
  const applySeek = () => {
    if (cancelled) {
      return;
    }
    const maxTime =
      Number.isFinite(video.duration) && video.duration > 0
        ? Math.max(video.duration - 0.1, 0)
        : targetSeconds;
    video.currentTime = Math.min(targetSeconds, maxTime);
    if (opts.shouldPlay) {
      void video.play().catch(() => {
        if (!cancelled) {
          opts.onPlayBlocked?.();
        }
      });
    }
  };

  if (video.readyState >= 1) {
    const frame = window.requestAnimationFrame(applySeek);
    return () => {
      cancelled = true;
      window.cancelAnimationFrame(frame);
    };
  }

  video.addEventListener("loadedmetadata", applySeek);
  return () => {
    cancelled = true;
    video.removeEventListener("loadedmetadata", applySeek);
  };
}

function usePlaybackPositionPersistence({
  itemId,
  videoRef,
  videoElement,
  chunkId,
  enabled,
}: {
  itemId: string;
  videoRef: RefObject<HTMLVideoElement | null>;
  videoElement?: HTMLVideoElement | null;
  chunkId: string | null;
  enabled: boolean;
}) {
  const lastSavedAtRef = useRef(0);
  const chunkIdRef = useRef(chunkId);

  useEffect(() => {
    chunkIdRef.current = chunkId;
  }, [chunkId]);

  useEffect(() => {
    if (!enabled) {
      return;
    }
    const video = videoElement ?? videoRef.current;
    if (!video) {
      return;
    }

    let disposed = false;
    const clearSavedPosition = () => {
      forgetLastOpened(itemId);
      void api
        .updatePlaybackPosition(itemId, 0, null)
        .catch((error) => console.warn("failed to clear playback position", error));
    };
    const persist = (force: boolean) => {
      if (disposed) {
        return;
      }
      const positionSec = video.currentTime;
      if (!Number.isFinite(positionSec) || positionSec < 1) {
        return;
      }
      if (isNearEndPosition(positionSec, video.duration)) {
        if (force) {
          clearSavedPosition();
        }
        return;
      }
      const now = Date.now();
      if (!force && now - lastSavedAtRef.current < 10_000) {
        return;
      }
      lastSavedAtRef.current = now;
      const timestamp = formatTimestamp(positionSec);
      recordLastOpened(itemId, timestamp);
      void api
        .updatePlaybackPosition(itemId, positionSec, chunkIdRef.current)
        .catch((error) => console.warn("failed to save playback position", error));
    };
    const persistThrottled = () => persist(false);
    const persistForced = () => persist(true);

    video.addEventListener("timeupdate", persistThrottled);
    video.addEventListener("pause", persistForced);
    video.addEventListener("ended", clearSavedPosition);
    window.addEventListener("pagehide", persistForced);
    return () => {
      persistForced();
      disposed = true;
      video.removeEventListener("timeupdate", persistThrottled);
      video.removeEventListener("pause", persistForced);
      video.removeEventListener("ended", clearSavedPosition);
      window.removeEventListener("pagehide", persistForced);
    };
  }, [enabled, itemId, videoElement, videoRef]);
}

async function revealSourcePath(path: string) {
  await invokeHostCommand("reveal_source_path", { path });
}

function parseTimeToSeconds(time: string): number {
  const parts = time.split(":").map((part) => Number.parseInt(part, 10) || 0);
  if (parts.length === 3) return parts[0] * 3600 + parts[1] * 60 + parts[2];
  if (parts.length === 2) return parts[0] * 60 + parts[1];
  return parts[0] ?? 0;
}

function secondsToSrtTimestamp(total: number): string {
  const pad = (value: number, width = 2) =>
    String(Math.max(0, Math.floor(value))).padStart(width, "0");
  return `${pad(total / 3600)}:${pad((total % 3600) / 60)}:${pad(total % 60)},000`;
}

function transcriptToSrt(lines: TranscriptLine[]): string {
  return lines
    .map((line, index) => {
      const start = parseTimeToSeconds(line.time);
      const nextStart =
        index + 1 < lines.length ? parseTimeToSeconds(lines[index + 1].time) : start + 3;
      const end = Math.max(nextStart, start + 1);
      return `${index + 1}\n${secondsToSrtTimestamp(start)} --> ${secondsToSrtTimestamp(end)}\n${line.text}`;
    })
    .join("\n\n");
}

function transcriptToMarkdown(title: string, lines: TranscriptLine[]): string {
  const body = lines.map((line) => `**[${line.time}]** ${line.text}`).join("\n\n");
  return `# ${title}\n\n${body}\n`;
}

function transcriptFilenameBase(title: string): string {
  const cleaned = title.replace(/[^\p{L}\p{N}\-_ ]/gu, "").trim().slice(0, 60);
  return cleaned || "transcript";
}

function downloadTextFile(filename: string, content: string, mime: string) {
  const blob = new Blob([content], { type: mime });
  const url = URL.createObjectURL(blob);
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = filename;
  document.body.appendChild(anchor);
  anchor.click();
  anchor.remove();
  window.setTimeout(() => URL.revokeObjectURL(url), 0);
}


// Overflow menu in the detail header: whole-transcript exports plus the
// lower-frequency maintenance actions (re-index, delete). Primary actions
// (copy citation, open source, export clip) stay as visible buttons.
function DetailActionsMenu({
  onExportMarkdown,
  onExportSrt,
  onReindex,
  onDelete,
  busy = false,
  reindexing = false,
  deleting = false,
}: {
  onExportMarkdown?: () => void;
  onExportSrt?: () => void;
  onReindex: () => void;
  onDelete: () => void;
  busy?: boolean;
  reindexing?: boolean;
  deleting?: boolean;
}) {
  const t = useT();
  const ref = useRef<HTMLDivElement | null>(null);
  const [open, setOpen] = useState(false);
  useEscapeToClose(() => setOpen(false), open);
  useClickOutside(ref, () => setOpen(false), open);
  const run = (fn: () => void) => {
    setOpen(false);
    fn();
  };
  return (
    <div className="row-actions" ref={ref}>
      <button
        className="btn-icon"
        type="button"
        aria-label={t("detail.moreActions")}
        aria-expanded={open}
        onClick={() => setOpen((value) => !value)}
      >
        <MoreHorizontal size={16} />
      </button>
      {open ? (
        <div className="menu row-menu" role="menu">
          {onExportMarkdown ? (
            <button type="button" onClick={() => run(onExportMarkdown)}>
              <Download size={15} />
              <span>{t("detail.action.exportMarkdown")}</span>
            </button>
          ) : null}
          {onExportSrt ? (
            <button type="button" onClick={() => run(onExportSrt)}>
              <Download size={15} />
              <span>{t("detail.action.exportSrt")}</span>
            </button>
          ) : null}
          <button type="button" disabled={busy} onClick={() => run(onReindex)}>
            {reindexing ? <Loader2 size={15} className="spin" /> : <RefreshCcw size={15} />}
            <span>{reindexing ? t("common.reindexing") : t("common.reindex")}</span>
          </button>
          <span className="msep" />
          <button className="danger" type="button" disabled={busy} onClick={() => run(onDelete)}>
            {deleting ? <Loader2 size={15} className="spin" /> : <Trash2 size={15} />}
            <span>{deleting ? t("common.deleting") : t("common.delete")}</span>
          </button>
        </div>
      ) : null}
    </div>
  );
}

function useItemMoments(item: Item, enabled: boolean) {
  const [moments, setMoments] = useState<api.MomentRecord[]>([]);
  const [pendingLineId, setPendingLineId] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);

  const reload = useCallback(async () => {
    if (!enabled) {
      setMoments([]);
      return;
    }
    const records = await api.listMoments();
    setMoments(records.filter((moment) => moment.item_id === item.id));
  }, [enabled, item.id]);

  useEffect(() => {
    let cancelled = false;
    if (!enabled) {
      setMoments([]);
      return;
    }
    api
      .listMoments()
      .then((records) => {
        if (!cancelled) {
          setMoments(records.filter((moment) => moment.item_id === item.id));
        }
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
    };
  }, [enabled, item.id]);

  // Indexed lookups: the per-line linear scan made transcript rendering
  // O(lines x moments).
  const momentIndex = useMemo(() => {
    const byChunk = new Map<string, api.MomentRecord>();
    const byQuote = new Map<string, api.MomentRecord>();
    for (const moment of moments) {
      if (moment.chunk_id) byChunk.set(moment.chunk_id, moment);
      byQuote.set(`${moment.timestamp}\u0000${moment.quote.trim()}`, moment);
    }
    return { byChunk, byQuote };
  }, [moments]);

  function momentForLine(line: TranscriptLine) {
    return (
      momentIndex.byChunk.get(line.id) ??
      momentIndex.byQuote.get(`${line.time}\u0000${line.text.trim()}`)
    );
  }

  async function toggle(line: TranscriptLine) {
    if (!enabled || pendingLineId) {
      return;
    }
    setPendingLineId(line.id);
    setMessage(null);
    try {
      const existing = momentForLine(line);
      if (existing) {
        await api.deleteMoment(existing.id);
      } else {
        const startSec = parseTimestampSeconds(line.time);
        await api.createMoment({
          item_id: item.id,
          chunk_id: line.id,
          start_sec: Number.isFinite(startSec) ? startSec : null,
          title: item.title,
          quote: line.text,
        });
      }
      await reload();
    } catch (error) {
      setMessage(errorMessage(error));
    } finally {
      setPendingLineId(null);
    }
  }

  return {
    moments,
    pendingLineId,
    message,
    momentForLine,
    toggle,
  };
}

function MomentLineAction({
  saved,
  pending,
  disabled,
  onToggle,
}: {
  saved: boolean;
  pending: boolean;
  disabled: boolean;
  onToggle: () => void;
}) {
  const t = useT();
  return (
    <button
      type="button"
      className={saved ? "moment-star saved" : "moment-star"}
      disabled={disabled || pending}
      title={saved ? t("moments.unsave") : t("moments.save")}
      aria-label={saved ? t("moments.unsave") : t("moments.save")}
      onClick={onToggle}
    >
      {pending ? <Loader2 size={14} className="spin" /> : <Star size={14} fill={saved ? "currentColor" : "none"} />}
    </button>
  );
}

function TranscriptReadingView({
  title,
  lines,
  onSeek,
}: {
  title: string;
  lines: TranscriptLine[];
  onSeek?: (timestamp: string) => void;
}) {
  return (
    <article className="transcript-reading">
      <h2 className="reading-title">{title}</h2>
      {lines.map((line) => (
        <p key={line.id} className="reading-para">
          <button
            type="button"
            className="reading-ts mono"
            onClick={() => onSeek?.(line.time)}
            aria-label={line.time}
          >
            {line.time}
          </button>
          <span>{line.text}</span>
        </p>
      ))}
    </article>
  );
}

function VideoUnderstandingPanel({
  item,
  enabled,
  onSeek,
  requestConfirm,
  onChapters,
  onUnderstanding,
  onAnalyzed,
  compactCompleted = false,
}: {
  item: Item;
  enabled: boolean;
  onSeek?: (timestamp: string) => void;
  requestConfirm: RequestConfirm;
  onChapters?: (chapters: api.VideoUnderstandingChapter[]) => void;
  // Reports the full understanding record whenever it changes (loaded / cleared)
  // so the parent (ItemDetail) can drive the summary, chapters, and frame strip
  // without issuing a second GET /items/{id}/understanding.
  onUnderstanding?: (record: api.VideoUnderstandingRecord | null) => void;
  onAnalyzed?: (record: api.VideoUnderstandingRecord) => void | Promise<void>;
  compactCompleted?: boolean;
}) {
  const t = useT();
  const [state, setState] = useState<{
    status: "idle" | "loading" | "analyzing" | "loaded" | "error";
    record: api.VideoUnderstandingRecord | null;
    message: string | null;
  }>({
    status: "idle",
    record: null,
    message: null,
  });
  // Tracks the currently displayed item so long-running requests started
  // for a previous item can detect they are stale.
  const itemIdRef = useRef(item.id);
  itemIdRef.current = item.id;
  const record = state.record?.item_id === item.id ? state.record : null;
  const isPending = state.status === "loading" || state.status === "analyzing";
  // Elapsed timer for the analyze run. The request is a single blocking call
  // (upload → Gemini processing → generate) with no server-side progress, so an
  // indeterminate bar + elapsed clock is the honest signal that work is ongoing.
  const [analyzeElapsedMs, setAnalyzeElapsedMs] = useState(0);

  useEffect(() => {
    if (!enabled || item.contentType !== "video") {
      setState({ status: "idle", record: null, message: null });
      return;
    }

    let cancelled = false;
    setState({ status: "loading", record: null, message: null });
    api
      .getItemUnderstanding(item.id)
      .then((next) => {
        if (!cancelled) {
          setState({ status: "loaded", record: next, message: null });
        }
      })
      .catch(() => {
        // A missing/unavailable understanding record is not a hard error for
        // this secondary panel — fall back to the "not analyzed" empty state
        // instead of flashing a red notice. Explicit Analyze failures below
        // still surface their message.
        if (!cancelled) {
          setState({ status: "idle", record: null, message: null });
        }
      });

    return () => {
      cancelled = true;
    };
  }, [enabled, item.contentType, item.id]);

  // Surface chapters to the host so the player can segment its timeline.
  useEffect(() => {
    onChapters?.(record?.chapters ?? []);
  }, [record, onChapters]);

  useEffect(() => {
    onUnderstanding?.(record ?? null);
  }, [record, onUnderstanding]);

  useEffect(() => {
    if (state.status !== "analyzing") {
      setAnalyzeElapsedMs(0);
      return;
    }
    const startedAt = performance.now();
    const interval = window.setInterval(() => {
      setAnalyzeElapsedMs(performance.now() - startedAt);
    }, 500);
    return () => window.clearInterval(interval);
  }, [state.status]);

  if (item.contentType !== "video") {
    return null;
  }

  async function analyze() {
    if (!enabled || isPending) {
      return;
    }
    const confirmed = await requestConfirm({
      title: t("understanding.confirm.title"),
      body: t("understanding.confirm.body"),
      confirmLabel: t("understanding.confirm.label"),
    });
    if (!confirmed) {
      return;
    }
    // The analyze POST can run for minutes while the panel stays mounted
    // across item switches; pin the id so a finished analysis for item A
    // can't be written into item B's panel (and its player chapters).
    const analyzedItemId = item.id;
    const isCurrent = () => analyzedItemId === itemIdRef.current;
    setState((current) => ({
      status: "analyzing",
      record: current.record,
      message: null,
    }));
    try {
      const next = await api.analyzeItemUnderstanding(analyzedItemId);
      if (!isCurrent()) return;
      setState({ status: "loaded", record: next, message: null });
      void Promise.resolve(onAnalyzed?.(next)).catch(() => undefined);
    } catch (error) {
      if (!isCurrent()) return;
      setState((current) => ({
        status: "error",
        record: current.record,
        message: errorMessage(error),
      }));
    }
  }

  const analysisStatus = record?.status ?? "not_started";
  const statusLabel =
    state.status === "loading"
      ? t("understanding.status.loading")
      : state.status === "analyzing"
        ? t("understanding.status.analyzing")
        : analysisStatus === "completed"
          ? t("understanding.status.analyzed")
          : analysisStatus === "failed"
            ? t("understanding.status.failed")
            : t("understanding.status.notAnalyzed");
  const statusChipClass =
    analysisStatus === "completed"
      ? "chip success"
      : analysisStatus === "failed"
        ? "chip danger"
        : state.status === "analyzing" || state.status === "loading"
          ? "chip accent"
          : "chip neutral";
  const hasUnderstandingContent =
    analysisStatus === "completed" &&
    Boolean(
      record?.summary?.trim() ||
        record?.chapters?.length ||
        record?.events?.length ||
        record?.topics?.length,
    );
  const summary = record?.summary?.trim() ?? "";
  const chapters = record?.chapters ?? [];
  const events = record?.events ?? [];
  const topics = record?.topics ?? [];
  const shouldRenderCompletedDetails = hasUnderstandingContent && !compactCompleted;
  const canAnalyze = enabled && !isPending;
  const privacyNote = t("understanding.privacyNote");

  return (
    <section className={`understanding-panel ${analysisStatus}`}>
      <div className="understanding-header">
        <div>
          <p className="section-label" style={{ marginBottom: 2 }}>{t("understanding.eyebrow")}</p>
          <strong>{t("understanding.title")}</strong>
        </div>
        <span className={statusChipClass}>
          <span className="dot" />
          {statusLabel}
        </span>
      </div>

      {state.message ? <InlineNotice tone="error" message={state.message} /> : null}
      {record?.error && analysisStatus === "failed" ? (
        <InlineNotice tone="error" message={record.error} />
      ) : null}

      {state.status === "analyzing" ? (
        <div className="understanding-progress" role="status" aria-live="polite">
          <div className="understanding-progress-track" aria-hidden="true">
            <span className="understanding-progress-fill" />
          </div>
          <div className="understanding-progress-meta">
            <span>{t("understanding.status.analyzing")}</span>
            <span className="mono faint">{formatTimestamp(Math.round(analyzeElapsedMs / 1000))}</span>
          </div>
          <p className="field-hint">{t("understanding.progress.hint")}</p>
        </div>
      ) : null}

      {state.status === "loading" ? (
        <div className="understanding-skeleton" aria-hidden="true">
          <span className="sk" />
          <span className="sk" />
        </div>
      ) : !hasUnderstandingContent ? (
        <p className="field-hint">{t("understanding.empty")}</p>
      ) : null}

      {shouldRenderCompletedDetails && summary ? (
        <p className="understanding-summary">{summary}</p>
      ) : null}

      {shouldRenderCompletedDetails && topics.length > 0 ? (
        <div className="understanding-topics" aria-label={t("understanding.topics.aria")}>
          {topics.slice(0, 8).map((topic) => (
            <span key={topic} className="chip neutral">{topic}</span>
          ))}
        </div>
      ) : null}

      <p className="field-hint">{privacyNote}</p>

      {shouldRenderCompletedDetails && chapters.length > 0 ? (
        <div className="understanding-list">
          <strong>{t("understanding.chapters")}</strong>
          {chapters.slice(0, 4).map((chapter, index) => (
            <button
              className="understanding-row"
              key={`${chapter.title}-${index}`}
              type="button"
              disabled={!onSeek}
              onClick={() =>
                chapter.start_sec !== null ? onSeek?.(formatTimestamp(chapter.start_sec)) : undefined
              }
            >
              <span className="kbd">{formatTimestamp(chapter.start_sec)}</span>
              <p>
                <b>{chapter.title}</b>
                {chapter.summary ? ` ${chapter.summary}` : ""}
              </p>
            </button>
          ))}
        </div>
      ) : null}

      {shouldRenderCompletedDetails && events.length > 0 ? (
        <div className="understanding-list">
          <strong>{t("understanding.keyMoments")}</strong>
          {events.slice(0, 5).map((event, index) => (
            <button
              className="understanding-row"
              key={`${event.caption}-${index}`}
              type="button"
              disabled={!onSeek}
              onClick={() =>
                event.start_sec !== null ? onSeek?.(formatTimestamp(event.start_sec)) : undefined
              }
            >
              <span className="kbd">{formatTimestamp(event.start_sec)}</span>
              <p>{event.caption}</p>
            </button>
          ))}
        </div>
      ) : null}

      <button
        type="button"
        className="btn btn-primary sm understanding-action"
        disabled={!canAnalyze}
        onClick={() => void analyze()}
      >
        {isPending ? <Loader2 size={15} className="spin" /> : <Sparkles size={15} />}
        <span>
          {isPending
            ? t("understanding.status.analyzing")
            : analysisStatus === "completed"
              ? t("understanding.action.reanalyze")
              : t("understanding.action.analyze")}
        </span>
      </button>
    </section>
  );
}

async function openOriginalSourceForItem(item: Item, t: TFunction) {
  if (item.originalUrl) {
    window.open(item.originalUrl, "_blank", "noopener,noreferrer");
    return t("detail.source.opened");
  }
  if (item.rawPath) {
    await revealSourcePath(item.rawPath);
    return t("detail.source.revealed");
  }
  throw new Error(t("detail.source.unavailable"));
}

export function ItemDetail({
  item,
  apiStatus,
  actionsEnabled,
  startTimestamp,
  startChunkId,
  onBack,
  onDeleteItem,
  onReindexItem,
  onItemUpdated,
  requestConfirm,
}: {
  item: Item;
  apiStatus: ApiStatus;
  actionsEnabled: boolean;
  startTimestamp: string;
  startChunkId: string | null;
  onBack: () => void;
  onDeleteItem: (item: Item) => Promise<void>;
  onReindexItem: (item: Item) => Promise<void>;
  onItemUpdated: () => Promise<void>;
  requestConfirm: RequestConfirm;
}) {
  const t = useT();
  const videoRef = useRef<HTMLVideoElement | null>(null);
  const [videoElement, setVideoElement] = useState<HTMLVideoElement | null>(null);
  const handleVideoElement = useCallback((video: HTMLVideoElement | null) => {
    setVideoElement(video);
  }, []);
  const [playerChapters, setPlayerChapters] = useState<PlayerChapter[]>([]);
  const handleUnderstandingChapters = useCallback((chapters: api.VideoUnderstandingChapter[]) => {
    setPlayerChapters(
      chapters
        .filter((chapter) => chapter.start_sec !== null)
        .map((chapter) => ({ seconds: chapter.start_sec as number, title: chapter.title })),
    );
  }, []);
  // The full understanding record, reported back from VideoUnderstandingPanel
  // via onUnderstanding. Drives the summary, chapters, and frame strip without
  // issuing a duplicate GET /items/{id}/understanding.
  const [understandingRecord, setUnderstandingRecord] =
    useState<api.VideoUnderstandingRecord | null>(null);
  const handleUnderstanding = useCallback(
    (record: api.VideoUnderstandingRecord | null) => {
      if (record && record.item_id !== item.id) {
        return;
      }
      setUnderstandingRecord(record);
    },
    [item.id],
  );
  const handleUnderstandingAnalyzed = useCallback(
    (record: api.VideoUnderstandingRecord) => {
      if (record.item_id === item.id) {
        setUnderstandingRecord(record);
      }
      void onItemUpdated();
    },
    [item.id, onItemUpdated],
  );
  const activeUnderstandingRecord =
    understandingRecord?.item_id === item.id ? understandingRecord : null;
  const understood =
    activeUnderstandingRecord?.status === "completed" &&
    Boolean(
      activeUnderstandingRecord.summary?.trim() ||
        activeUnderstandingRecord.chapters.length ||
        activeUnderstandingRecord.events.length ||
        activeUnderstandingRecord.topics.length,
    );
  const detailTitle =
    activeUnderstandingRecord?.display_title?.trim() || item.title;
  const modalityLabel = itemModalityLabel(item, t);
  const [currentTimestamp, setCurrentTimestamp] = useState(startTimestamp);
  const [currentPlayheadSec, setCurrentPlayheadSec] = useState(() =>
    parseTimestampSeconds(startTimestamp),
  );
  const [chunkState, setChunkState] = useState<{
    status: "idle" | "loading" | "loaded" | "error";
    records: api.ChunkRecord[];
    lines: TranscriptLine[];
    message: string | null;
  }>({
    status: "idle",
    records: [],
    lines: transcript,
    message: null,
  });
  const [itemAction, setItemAction] = useState<{
    status: "idle" | "locating" | "reindexing" | "deleting" | "queued" | "error";
    message: string | null;
  }>({ status: "idle", message: null });
  const detailIssue = itemDetailIssue(item, t);
  const transcriptLines =
    apiStatus === "online" && chunkState.status !== "idle" ? chunkState.lines : transcript;
  const momentActions = useItemMoments(
    item,
    actionsEnabled && chunkState.status === "loaded",
  );
  const playerMarkers: PlayerMarker[] = useMemo(
    () =>
      transcriptLines
        .map((line) => ({
          seconds: parseTimestampSeconds(line.time),
          label: line.time,
          text: line.text,
        }))
        .filter((marker) => Number.isFinite(marker.seconds) && marker.seconds >= 0),
    [transcriptLines],
  );
  // Show a real inline video player whenever we have any chunk to point
  // at: prefer the existing thumbnail chunk (so we can use the same chunk
  // id used for the keyframe), otherwise use the first transcript line.
  const playableChunkId =
    item.contentType === "video"
      ? chunkState.status === "loaded"
        ? selectPlaybackChunkId(
            chunkState.records,
            startTimestamp,
            startChunkId ?? extractChunkIdFromThumbnail(item.thumbnailUrl),
          )
        : startChunkId
          ? null
          : extractChunkIdFromThumbnail(item.thumbnailUrl)
      : null;
  const itemPlaybackUrl = playableChunkId ? api.videoSegmentUrl(playableChunkId) : null;

  const [copyStatus, setCopyStatus] = useState<"idle" | "copied" | "error">("idle");
  const itemBusy =
    itemAction.status === "reindexing" ||
    itemAction.status === "deleting" ||
    itemAction.status === "locating";
  const timestampLink = timestampDeepLink(item.id, currentTimestamp, playableChunkId, "item-detail");
  const handlePlayerTimeUpdate = useCallback((seconds: number) => {
    if (!Number.isFinite(seconds) || seconds < 0) {
      return;
    }
    setCurrentPlayheadSec((current) =>
      Math.abs(current - seconds) < 0.1 ? current : seconds,
    );
    const timestamp = formatTimestamp(seconds);
    setCurrentTimestamp((current) => (current === timestamp ? current : timestamp));
  }, []);
  // Resolve the chunk to clip from the LIVE playhead when the export popover
  // opens (falls back to currentTimestamp / the thumbnail chunk).
  function resolveClipTarget(): ClipTarget | null {
    const targetSec = Number.isFinite(currentPlayheadSec)
      ? currentPlayheadSec
      : parseTimestampSeconds(currentTimestamp);
    return resolveClipTarget_(transcriptLines, targetSec);
  }

  useEffect(() => {
    if (copyStatus === "idle") {
      return;
    }
    const timeout = window.setTimeout(() => setCopyStatus("idle"), 1600);
    return () => window.clearTimeout(timeout);
  }, [copyStatus]);

  usePlaybackPositionPersistence({
    itemId: item.id,
    videoRef,
    videoElement,
    chunkId: playableChunkId,
    enabled: actionsEnabled && Boolean(itemPlaybackUrl),
  });

  useEffect(() => {
    setItemAction({ status: "idle", message: null });
    setUnderstandingRecord(null);
    setPlayerChapters([]);
  }, [item.id]);

  useEffect(() => {
    setCurrentTimestamp(startTimestamp);
    setCurrentPlayheadSec(parseTimestampSeconds(startTimestamp));
  }, [item.id, startTimestamp]);

  useEffect(() => {
    const video = videoElement;
    if (!video || !itemPlaybackUrl) {
      return;
    }

    return syncVideoToTimestamp(video, startTimestamp, {
      shouldPlay: parseTimestampSeconds(startTimestamp) > 0,
    });
  }, [item.id, itemPlaybackUrl, startTimestamp, videoElement]);

  useEffect(() => {
    function onKeyDown(event: globalThis.KeyboardEvent) {
      if (hasOpenModalSurface()) {
        return;
      }
      if (event.key === "Escape") {
        event.preventDefault();
        onBack();
        return;
      }
      const target = event.target as HTMLElement | null;
      if (
        target &&
        (target.tagName === "INPUT" ||
          target.tagName === "TEXTAREA" ||
          target.tagName === "BUTTON" ||
          target.tagName === "A" ||
          target.tagName === "SELECT" ||
          target.tagName === "VIDEO" ||
          target.isContentEditable ||
          target.getAttribute("role") === "button")
      ) {
        return;
      }
      const video = videoRef.current;
      if (!video) {
        return;
      }
      if (event.key === " " || event.code === "Space") {
        event.preventDefault();
        if (video.paused) {
          void video.play().catch(() => undefined);
        } else {
          video.pause();
        }
      } else if (event.key === "ArrowRight") {
        event.preventDefault();
        video.currentTime = Math.min(video.duration || Number.POSITIVE_INFINITY, video.currentTime + 5);
      } else if (event.key === "ArrowLeft") {
        event.preventDefault();
        video.currentTime = Math.max(0, video.currentTime - 5);
      } else if (event.key === "ArrowUp") {
        event.preventDefault();
        video.volume = Math.min(1, video.volume + 0.1);
      } else if (event.key === "ArrowDown") {
        event.preventDefault();
        video.volume = Math.max(0, video.volume - 0.1);
      } else if (event.key.toLowerCase() === "m") {
        video.muted = !video.muted;
      } else if (event.key.toLowerCase() === "f") {
        if (document.fullscreenElement) {
          void document.exitFullscreen().catch(() => undefined);
        } else if (video.requestFullscreen) {
          void video.requestFullscreen().catch(() => undefined);
        }
      }
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [onBack]);

  useEffect(() => {
    if (apiStatus !== "online") {
      setChunkState({ status: "idle", records: [], lines: transcript, message: null });
      return;
    }

    let cancelled = false;
    setChunkState({ status: "loading", records: [], lines: [], message: null });
    api
      .listItemChunks(item.id)
      .then((records) => {
        if (cancelled) {
          return;
        }
        setChunkState({
          status: "loaded",
          records,
          lines: mapChunkRecords(records),
          message: null,
        });
      })
      .catch((error) => {
        if (cancelled) {
          return;
        }
        setChunkState({ status: "error", records: [], lines: [], message: errorMessage(error) });
      });

    return () => {
      cancelled = true;
    };
  }, [apiStatus, item.id]);

  async function locateSourceFile() {
    setItemAction({ status: "locating", message: null });
    const selected = await openDialog({
      multiple: false,
      directory: false,
      filters: [{ name: "Video", extensions: ["mp4", "mkv", "webm", "mov", "m4v"] }],
    }).catch(() => null);
    if (typeof selected === "string" && selected.trim()) {
      try {
        await api.updateItemRawPath(item.id, selected.trim());
        await onItemUpdated();
        setItemAction({
          status: "idle",
          message: t("detail.locatedSource", { path: selected }),
        });
      } catch (error) {
        setItemAction({ status: "error", message: errorMessage(error) });
      }
      return;
    }
    setItemAction({ status: "idle", message: null });
  }

  async function openOriginalSource() {
    if (!canOpenOriginalSource(item)) {
      return;
    }
    if (!item.originalUrl) {
      setItemAction({ status: "locating", message: null });
    }
    try {
      const message = await openOriginalSourceForItem(item, t);
      if (!item.originalUrl) {
        setItemAction({ status: "queued", message });
      }
    } catch (error) {
      setItemAction({ status: "error", message: errorMessage(error) });
    }
  }

  async function reindexCurrentItem() {
    if (!actionsEnabled) {
      setItemAction({ status: "error", message: t("common.coreUnreachable") });
      return;
    }

    const confirmed = await requestConfirm({
      title: t("common.confirm.reindex.title"),
      body: t("common.confirm.reindex.body"),
      confirmLabel: t("common.reindex"),
    });
    if (!confirmed) {
      return;
    }

    setItemAction({ status: "reindexing", message: null });
    try {
      await onReindexItem(item);
      setItemAction({ status: "queued", message: t("common.reindexQueued") });
    } catch (error) {
      setItemAction({ status: "error", message: errorMessage(error) });
    }
  }

  async function deleteCurrentItem() {
    if (!actionsEnabled) {
      setItemAction({ status: "error", message: t("common.coreUnreachable") });
      return;
    }
    const confirmed = await requestConfirm({
      title: t("common.confirm.delete.title"),
      body: t("common.confirm.delete.body", { title: item.title }),
      confirmLabel: t("common.delete"),
    });
    if (!confirmed) {
      return;
    }

    setItemAction({ status: "deleting", message: null });
    try {
      await onDeleteItem(item);
    } catch (error) {
      setItemAction({ status: "error", message: errorMessage(error) });
    }
  }

  async function copyCitation() {
    try {
      const quote = transcriptLines.find((line) => line.time === currentTimestamp)?.text;
      const citation = buildMomentCitation({
        title: item.title,
        timestamp: currentTimestamp,
        quote,
        link: item.originalUrl ?? timestampLink,
      });
      await writeClipboardText(citation);
      setCopyStatus("copied");
    } catch (error) {
      // Surface the failure (the header button has no error affordance, so a
      // failed copy used to look like nothing happened).
      setCopyStatus("error");
      setItemAction({ status: "error", message: errorMessage(error) });
    }
  }


  // Seek the inline player to a timestamp. The /video-segment endpoint serves the
  // full source video with Range support, so the loaded src is the whole file.
  // This drives the transcript rows, chapters, and key moments.
  function seekTo(timestamp: string) {
    const targetSeconds = parseTimestampSeconds(timestamp);
    if (!Number.isFinite(targetSeconds)) {
      return;
    }
    setCurrentTimestamp(timestamp);
    setCurrentPlayheadSec(targetSeconds);
    const video = videoRef.current;
    if (!video) {
      return;
    }
    const applySeek = () => {
      const maxTime = Number.isFinite(video.duration)
        ? Math.max(video.duration - 0.1, 0)
        : targetSeconds;
      video.currentTime = Math.min(targetSeconds, maxTime);
      void video.play().catch(() => undefined);
    };
    if (video.readyState >= 1) {
      applySeek();
    } else {
      video.addEventListener("loadedmetadata", applySeek, { once: true });
    }
  }

  return (
    <div className="page wide">
      <div className="page-head">
        <button className="btn btn-ghost sm" type="button" onClick={onBack}>
          <ChevronRight size={15} style={{ transform: "rotate(180deg)" }} />
          <span>{t("library.heading")}</span>
        </button>
        <div
          className="row"
          style={{ alignItems: "flex-start", justifyContent: "space-between", gap: 12, marginTop: 12 }}
        >
          <div style={{ minWidth: 0 }}>
            <div style={{ display: "flex", alignItems: "center", gap: 10, flexWrap: "wrap" }}>
              <h1 className="page-h1">{detailTitle}</h1>
              {/* Source file / BV id — quiet mono chip so the original mapping is
                  never lost, never the lead. */}
              {item.rawPath ? (
                <span className="chip neutral" title={t("dt.source.file")}>
                  <span className="mono">{basenameFromPath(item.rawPath) ?? item.rawPath}</span>
                </span>
              ) : null}
            </div>
            {/* One inline subtitle (source · duration · searchable · indexed),
                replacing the old 6-row table that exposed chunk count / model
                / per-item $. */}
            <p className="page-sub">
              {item.source} · <span className="mono">{item.duration}</span> ·{" "}
              {modalityLabel} ·{" "}
              {item.indexedAtEpoch === null
                ? t("detail.notIndexed")
                : t("detail.indexedAt", { when: item.indexedAt })}
            </p>
          </div>
          <div className="row gap-2" style={{ flex: "none" }}>
            <button className="btn btn-ghost sm" type="button" onClick={() => void copyCitation()}>
              {copyStatus === "copied" ? <Check size={15} /> : <Copy size={15} />}
              <span>{copyStatus === "copied" ? t("detail.copy.copied") : t("detail.copy.label")}</span>
            </button>
            <button
              className="btn btn-secondary sm"
              type="button"
              disabled={!canOpenOriginalSource(item) || itemBusy}
              onClick={() => void openOriginalSource()}
            >
              {item.originalUrl ? <ExternalLink size={15} /> : <Folder size={15} />}
              <span>{item.originalUrl ? t("detail.source.openOriginal") : t("detail.source.reveal")}</span>
            </button>
            <ClipExportButton
              contentType={item.contentType}
              disabled={itemBusy}
              resolveTarget={resolveClipTarget}
            />
            <DetailActionsMenu
              onExportMarkdown={
                transcriptLines.length > 0
                  ? () =>
                      downloadTextFile(
                        `${transcriptFilenameBase(detailTitle)}.md`,
                        transcriptToMarkdown(detailTitle, transcriptLines),
                        "text/markdown;charset=utf-8",
                      )
                  : undefined
              }
              onExportSrt={
                transcriptLines.length > 0
                  ? () =>
                      downloadTextFile(
                        `${transcriptFilenameBase(detailTitle)}.srt`,
                        transcriptToSrt(transcriptLines),
                        "text/plain;charset=utf-8",
                      )
                  : undefined
              }
              onReindex={() => void reindexCurrentItem()}
              onDelete={() => void deleteCurrentItem()}
              busy={itemBusy}
              reindexing={itemAction.status === "reindexing"}
              deleting={itemAction.status === "deleting"}
            />
          </div>
        </div>
      </div>

      {/* ===== Scheme B: summary above, player+chapters next to transcript,
          keyframes below the main viewing stage. Empty data does not render
          placeholder chrome. */}
      {understood ? (
        <SummaryCard
          summary={activeUnderstandingRecord?.summary ?? null}
          topics={activeUnderstandingRecord?.topics ?? []}
        />
      ) : null}

      <SplitStage
        currentSec={currentPlayheadSec}
        chapters={activeUnderstandingRecord?.chapters ?? []}
        onSeek={seekTo}
        understood={understood}
        left={
          /* The exact chrome that used to live in `.detail-media`: issue panel,
             or the live CerulPlayer, or the placeholder big play-button. */
          detailIssue ? (
            <div className="detail-media-issue">
              <DetailIssuePanel
                issue={detailIssue}
                actionStatus={itemAction.status}
                actionsEnabled={actionsEnabled}
                hasOriginalUrl={Boolean(item.originalUrl)}
                onLocate={() => void locateSourceFile()}
                onOpenOriginal={() => void openOriginalSource()}
                onReindex={() => void reindexCurrentItem()}
                onRemove={() => void deleteCurrentItem()}
              />
            </div>
          ) : itemPlaybackUrl ? (
            <CerulPlayer
              videoRef={videoRef}
              src={itemPlaybackUrl}
              markers={playerMarkers}
              chapters={playerChapters}
              ariaLabel={t("itemDetail.player.aria", { title: detailTitle })}
              fallbackDurationSec={item.durationSec}
              onSeekMarker={(marker) => seekTo(marker.label)}
              onTimeUpdate={handlePlayerTimeUpdate}
              onVideoElement={handleVideoElement}
            />
          ) : (
            <div className={`video-frame ${item.color}`}>
              <button
                className="play-button"
                type="button"
                aria-label={
                  item.status === "indexing"
                    ? t("itemDetail.player.waitingAria")
                    : t("itemDetail.player.noChunkAria")
                }
                disabled
              >
                <Play size={24} fill="currentColor" />
              </button>
            </div>
          )
        }
        right={
          /* The exact right rail that used to live in `.detail-transcript`:
             understanding panel + notices + transcript. */
          <>
            <VideoUnderstandingPanel
              item={item}
              enabled={actionsEnabled}
              onSeek={seekTo}
              requestConfirm={requestConfirm}
              onChapters={handleUnderstandingChapters}
              onUnderstanding={handleUnderstanding}
              onAnalyzed={handleUnderstandingAnalyzed}
              compactCompleted
            />
            {itemAction.message ? (
              <p
                className={itemAction.status === "error" ? "field-error" : "field-hint"}
                role="status"
              >
                {itemAction.message}
              </p>
            ) : null}
            {momentActions.message ? <InlineNotice tone="error" message={momentActions.message} /> : null}
            {chunkState.status === "loading" ? <TranscriptSkeleton /> : null}
            {chunkState.status === "error" && chunkState.message ? (
              <InlineNotice tone="error" message={chunkState.message} />
            ) : null}
            {chunkState.status === "loaded" &&
            transcriptLines.length === 0 &&
            item.status === "indexing" ? (
              <InlineNotice tone="muted" message={t("detail.stillProcessing")} />
            ) : null}
            {item.visualIndexMessage ? (
              <InlineNotice tone="muted" message={item.visualIndexMessage} />
            ) : null}
            {item.embeddingIndexMessage ? (
              <InlineNotice tone="muted" message={item.embeddingIndexMessage} />
            ) : null}
            {chunkState.status !== "loading" && transcriptLines.length > 0 ? (
              <TranscriptList
                lines={transcriptLines}
                videoRef={videoRef}
                videoReady={Boolean(itemPlaybackUrl)}
                activeTime={currentTimestamp}
                onSeek={seekTo}
                renderAction={(line) => {
                  const saved = Boolean(momentActions.momentForLine(line));
                  return (
                    <MomentLineAction
                      saved={saved}
                      pending={momentActions.pendingLineId === line.id}
                      disabled={!actionsEnabled}
                      onToggle={() => void momentActions.toggle(line)}
                    />
                  );
                }}
              />
            ) : null}
          </>
        }
      />
      <FrameStrip
        events={activeUnderstandingRecord?.events ?? []}
        chapters={activeUnderstandingRecord?.chapters ?? []}
        chunks={chunkState.records}
        durationSec={item.durationSec}
        currentTime={currentPlayheadSec}
        understood={understood}
        onSeek={seekTo}
      />
    </div>
  );
}
