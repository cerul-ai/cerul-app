// Transcript list + loading skeleton. Extracted from App.tsx (B13 Phase B).

import { CircleDot } from "lucide-react";
import type { ReactNode } from "react";
import { useT } from "../lib/i18n";
import type { TranscriptLine } from "../lib/types";

export function TranscriptList({
  lines,
  activeTime = "12:34",
  matchTime,
  onSeek,
}: {
  lines: TranscriptLine[];
  activeTime?: string;
  matchTime?: string;
  onSeek?: (timestamp: string) => void;
}) {
  return (
    <div className="seg-line transcript">
      {lines.map((line) => (
        <button
          key={line.id}
          type="button"
          className={[
            "seg-btn",
            line.time === activeTime ? "selected hot" : "",
            line.time === matchTime ? "accent matched" : "",
          ].filter(Boolean).join(" ")}
          onClick={() => onSeek?.(line.time)}
        >
          <span className="ts mono">
            {line.time === matchTime ? <CircleDot size={12} /> : null}
            {line.time}
          </span>
          <p className="seg-text">{line.text}</p>
        </button>
      ))}
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
