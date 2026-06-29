// Draggable two-pane split for the ItemDetail redesign.
//
// Left pane (60% by default): the CerulPlayer + chapter seek list (only
// shown when video understanding produced chapters). Right pane: the right
// rail's existing children — VideoUnderstandingPanel + transcript / notices.
// A thin splitter in the middle (a dot handle, col-resize cursor) is an
// explicit affordance so users discover they can resize; drag range is
// clamped so the right pane never collapses and the left pane stays
// comfortably large.

import { useCallback, useRef, useState } from "react";
import { Mic } from "lucide-react";
import { useT } from "../lib/i18n";
import { formatTimestamp } from "../lib/formatters";
import type * as api from "../lib/api";

type SplitStageProps = {
  // seconds for the chapter seek highlight
  currentSec: number;
  chapters: api.VideoUnderstandingChapter[];
  onSeek: (timestamp: string) => void;
  understood: boolean;
  // left pane: rendered by parent so it can pass its existing CerulPlayer props
  // (videoRef, src, markers, chapters, fallbackDurationSec, onSeekMarker,
  // onVideoElement) without re-declaring them here.
  left: React.ReactNode;
  // right pane: anything the parent puts in the existing detail-transcript
  // column (VideoUnderstandingPanel + transcript + notices + skeleton).
  right: React.ReactNode;
};

export function SplitStage({
  currentSec,
  chapters,
  onSeek,
  understood,
  left,
  right,
}: SplitStageProps) {
  const t = useT();
  const ref = useRef<HTMLDivElement | null>(null);
  // 0.6 = left starts big (60%) per the design round-4 default.
  const [leftPct, setLeftPct] = useState(0.6);
  const [dragging, setDragging] = useState(false);

  const startDrag = useCallback((event: React.MouseEvent<HTMLDivElement>) => {
    event.preventDefault();
    setDragging(true);

    const onMove = (ev: MouseEvent) => {
      const el = ref.current;
      if (!el) {
        return;
      }
      const rect = el.getBoundingClientRect();
      // The splitter occupies grid column 2 (6px wide); place the cursor
      // measured against the parent's total width.
      let pct = (ev.clientX - rect.left) / rect.width;
      const minRightPx = 280;
      const maxLeftPct = rect.width > minRightPx * 2
        ? 1 - minRightPx / rect.width
        : 0.58;
      pct = Math.max(0.34, Math.min(Math.min(0.76, maxLeftPct), pct));
      setLeftPct(pct);
    };
    const onUp = () => {
      setDragging(false);
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
    };
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
  }, []);

  return (
    <div
      ref={ref}
      className="splitstage"
      style={{
        gridTemplateColumns: `minmax(0, calc(${leftPct * 100}% - 3px)) 6px minmax(280px, calc(${
          (1 - leftPct) * 100
        }% - 3px))`,
      }}
    >
      {/* Left pane: caller-provided player chrome + (optional) chapter seek */}
      <div className="pane pane-left">
        {left}
        {understood && chapters.length > 0 ? (
          <div className="card split-chapters">
            <div className="split-chapters-head">
              <span className="strip-label">{t("dt.chapters.direct")}</span>
            </div>
            <div className="split-chapters-list">
              {chapters.map((c, i) => {
                const next = chapters[i + 1];
                const isCurrent =
                  currentSec >= (c.start_sec ?? 0) &&
                  (!next || currentSec < (next.start_sec ?? 0));
                return (
                  <button
                    key={i}
                    type="button"
                    className={`chap-btn${isCurrent ? " active" : ""}`}
                    onClick={() =>
                      c.start_sec !== null
                        ? onSeek(formatTimestamp(c.start_sec))
                        : undefined
                    }
                  >
                    <span className="ts mono">
                      {c.start_sec !== null ? formatTimestamp(c.start_sec) : "--:--"}
                    </span>
                    <span className="chap-body">
                      <b>{c.title}</b>
                      {c.summary ? <span className="chap-sum">{c.summary}</span> : null}
                    </span>
                  </button>
                );
              })}
            </div>
          </div>
        ) : null}
      </div>

      {/* splitter — a thin line with a dot handle, explicit col-resize affordance */}
      <div
        className={`splitter${dragging ? " active" : ""}`}
        onMouseDown={startDrag}
        title={t("dt.split.resize")}
        aria-hidden="true"
      />

      {/* Right pane: caller-provided right rail (transcript + panel + notices) */}
      <div className="pane pane-right">
        <div className="pane-right-head">
          <Mic size={13} />
          <span className="strip-label">{t("dt.split.transcript")}</span>
        </div>
        {right}
      </div>
    </div>
  );
}
