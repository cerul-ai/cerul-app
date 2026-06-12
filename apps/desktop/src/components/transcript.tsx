// Transcript list + loading skeleton. Extracted from App.tsx (B13 Phase B).

import { useEffect, useState } from "react";
import { CircleDot } from "lucide-react";
import type { ReactNode, RefObject } from "react";
import { parseTimestampSeconds } from "../lib/formatters";
import { useT } from "../lib/i18n";
import type { TranscriptLine } from "../lib/types";

export function TranscriptList({
  lines,
  videoRef,
  videoReady = false,
  activeTime,
  matchTime,
  onSeek,
  renderAction,
}: {
  lines: TranscriptLine[];
  videoRef?: RefObject<HTMLVideoElement | null>;
  videoReady?: boolean;
  activeTime?: string;
  matchTime?: string;
  onSeek?: (timestamp: string) => void;
  renderAction?: (line: TranscriptLine) => ReactNode;
}) {
  // Follow the playhead: the active line is the last one whose start is at or
  // before the current play position. Tracked here (not in the parent) so the
  // big detail view doesn't re-render on every timeupdate, and we only re-render
  // when the highlighted line actually changes.
  const [activeId, setActiveId] = useState<string | null>(null);
  useEffect(() => {
    const video = videoRef?.current;
    if (!video) {
      setActiveId(null);
      return;
    }
    const recompute = () => {
      const seconds = video.currentTime;
      let id: string | null = null;
      let best = -1;
      for (const line of lines) {
        const start = parseTimestampSeconds(line.time);
        if (Number.isFinite(start) && start <= seconds + 0.05 && start >= best) {
          best = start;
          id = line.id;
        }
      }
      setActiveId((prev) => (prev === id ? prev : id));
    };
    recompute();
    video.addEventListener("timeupdate", recompute);
    video.addEventListener("seeking", recompute);
    return () => {
      video.removeEventListener("timeupdate", recompute);
      video.removeEventListener("seeking", recompute);
    };
  }, [videoRef, videoReady, lines]);

  return (
    <div className="seg-line transcript">
      {lines.map((line) => {
        // Prefer the live playhead; fall back to activeTime (e.g. before the
        // video is ready, or in fixtures with no real playback).
        const isActive = activeId ? line.id === activeId : line.time === activeTime;
        return (
          <div
            key={line.id}
            className={[
              "seg-btn",
              isActive ? "selected hot" : "",
              line.time === matchTime ? "accent matched" : "",
            ].filter(Boolean).join(" ")}
          >
            <button
              type="button"
              className="seg-btn-main"
              onClick={() => onSeek?.(line.time)}
            >
              <span className="ts mono">
                {line.time === matchTime ? <CircleDot size={12} /> : null}
                {line.time}
              </span>
              <p className="seg-text">{line.text}</p>
            </button>
            {renderAction ? <span className="seg-action">{renderAction(line)}</span> : null}
          </div>
        );
      })}
    </div>
  );
}

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
