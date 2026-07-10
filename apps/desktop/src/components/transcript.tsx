// Transcript list + loading skeleton. Extracted from App.tsx (B13 Phase B).

import { memo, useEffect, useMemo, useState } from "react";
import { CircleDot } from "lucide-react";
import type { ReactNode, RefObject } from "react";
import { parseTimestampSeconds } from "../lib/formatters";
import { useT } from "../lib/i18n";
import type { TranscriptLine } from "../lib/types";

export function TranscriptList({
  lines,
  videoRef,
  audioRef,
  videoReady = false,
  activeTime,
  matchTime,
  onSeek,
  renderAction,
}: {
  lines: TranscriptLine[];
  videoRef?: RefObject<HTMLVideoElement | null>;
  audioRef?: RefObject<HTMLAudioElement | null>;
  videoReady?: boolean;
  activeTime?: string;
  matchTime?: string;
  onSeek?: (timestamp: string, line?: TranscriptLine) => void;
  renderAction?: (line: TranscriptLine) => ReactNode;
}) {
  // Follow the playhead: the active line is the last one whose start is at or
  // before the current play position. Tracked here (not in the parent) so the
  // big detail view doesn't re-render on every timeupdate, and we only re-render
  // when the highlighted line actually changes.
  const [activeId, setActiveId] = useState<string | null>(null);
  // Timestamps parsed once per transcript, not once per line per timeupdate.
  const lineStarts = useMemo(
    () => lines.map((line) => ({ id: line.id, start: parseTimestampSeconds(line.time) })),
    [lines],
  );
  useEffect(() => {
    const media = audioRef?.current ?? videoRef?.current;
    if (!media) {
      setActiveId(null);
      return;
    }
    const recompute = () => {
      const seconds = media.currentTime;
      let id: string | null = null;
      let best = -1;
      for (const entry of lineStarts) {
        if (Number.isFinite(entry.start) && entry.start <= seconds + 0.05 && entry.start >= best) {
          best = entry.start;
          id = entry.id;
        }
      }
      setActiveId((prev) => (prev === id ? prev : id));
    };
    recompute();
    media.addEventListener("timeupdate", recompute);
    media.addEventListener("seeking", recompute);
    return () => {
      media.removeEventListener("timeupdate", recompute);
      media.removeEventListener("seeking", recompute);
    };
  }, [audioRef, videoRef, videoReady, lineStarts]);

  return (
    <div className="seg-line transcript">
      {lines.map((line) => (
        <TranscriptRow
          key={line.id}
          line={line}
          // Prefer the live playhead; fall back to activeTime (e.g. before the
          // video is ready, or in fixtures with no real playback).
          isActive={activeId ? line.id === activeId : line.time === activeTime || line.id === activeTime}
          isMatch={line.time === matchTime || line.id === matchTime}
          onSeek={onSeek}
          renderAction={renderAction}
        />
      ))}
    </div>
  );
}

// Memoized so a playhead move re-renders only the rows whose highlight
// changed instead of reconciling thousands of rows per timeupdate.
const TranscriptRow = memo(function TranscriptRow({
  line,
  isActive,
  isMatch,
  onSeek,
  renderAction,
}: {
  line: TranscriptLine;
  isActive: boolean;
  isMatch: boolean;
  onSeek?: (timestamp: string, line?: TranscriptLine) => void;
  renderAction?: (line: TranscriptLine) => ReactNode;
}) {
  const displayTime = line.displayTime ?? line.time;
  return (
    <div
      data-line-id={line.id}
      className={["seg-btn", isActive ? "selected hot" : "", isMatch ? "accent matched" : ""]
        .filter(Boolean)
        .join(" ")}
    >
      <button type="button" className="seg-btn-main" onClick={() => onSeek?.(line.time, line)}>
        <span className="ts mono">
          {isMatch ? <CircleDot size={12} /> : null}
          {displayTime}
        </span>
        <p className="seg-text">{line.text}</p>
      </button>
      {renderAction ? <span className="seg-action">{renderAction(line)}</span> : null}
    </div>
  );
});

export function TranscriptSkeleton() {
  const t = useT();
  return (
    <div
      className="seg-line transcript transcript-skeleton"
      aria-label={t("transcript.loadingAria")}
    >
      {[0, 1, 2].map((index) => (
        <span key={index} className="sk" />
      ))}
    </div>
  );
}

export function StatusBadge({ status, label }: { status: string; label: string }) {
  return (
    <span className={`chip status-badge ${status}`}>
      <span className="dot" />
      {label}
    </span>
  );
}

export function ProgressBar({ value, animated = false }: { value: number; animated?: boolean }) {
  return (
    <span className={`progress${animated ? " animated" : ""}`}>
      <span className="bar" style={{ width: `${value}%` }} />
    </span>
  );
}

export function highlightSnippet(snippet: string, phrase: string): ReactNode {
  const normalizedPhrase = phrase.trim();
  if (!normalizedPhrase) {
    return snippet;
  }

  const index = snippet.toLowerCase().indexOf(normalizedPhrase.toLowerCase());
  if (index < 0) {
    return snippet;
  }
  return (
    <>
      {snippet.slice(0, index)}
      <mark>{snippet.slice(index, index + normalizedPhrase.length)}</mark>
      {snippet.slice(index + normalizedPhrase.length)}
    </>
  );
}
