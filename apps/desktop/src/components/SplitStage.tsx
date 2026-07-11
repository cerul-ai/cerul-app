// P1 detail workbench: independently scrollable chapters and transcript with
// two draggable separators around a stable player/citation stage.

import { useRef, useState } from "react";
import type { CSSProperties, PointerEvent as ReactPointerEvent } from "react";
import { GripVertical, Mic } from "lucide-react";
import { useT } from "../lib/i18n";
import { formatTimestamp } from "../lib/formatters";
import type * as api from "../lib/api";

type SplitStageProps = {
  currentSec: number;
  chapters: api.VideoUnderstandingChapter[];
  onSeek: (timestamp: string) => void;
  understood: boolean;
  left: React.ReactNode;
  right: React.ReactNode;
  under?: React.ReactNode;
};

export function SplitStage({
  currentSec,
  chapters,
  onSeek,
  understood,
  left,
  right,
  under,
}: SplitStageProps) {
  const t = useT();
  const showChapters = understood && chapters.length > 0;
  const stageRef = useRef<HTMLDivElement | null>(null);
  const [leftWidth, setLeftWidth] = useState(228);
  const [rightWidth, setRightWidth] = useState(354);

  function beginResize(side: "left" | "right", event: ReactPointerEvent<HTMLDivElement>) {
    const stage = stageRef.current;
    if (!stage) return;
    event.preventDefault();
    event.currentTarget.setPointerCapture(event.pointerId);
    const startX = event.clientX;
    const startLeft = leftWidth;
    const startRight = rightWidth;
    const bounds = stage.getBoundingClientRect();
    const onMove = (moveEvent: PointerEvent) => {
      const delta = moveEvent.clientX - startX;
      if (side === "left") {
        const maxLeft = Math.max(220, bounds.width - rightWidth - 500);
        setLeftWidth(Math.min(maxLeft, Math.max(190, startLeft + delta)));
      } else {
        const maxRight = Math.max(320, bounds.width - (showChapters ? leftWidth : 0) - 500);
        setRightWidth(Math.min(maxRight, Math.max(300, startRight - delta)));
      }
    };
    const onUp = () => {
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onUp);
    };
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp, { once: true });
  }

  function nudge(side: "left" | "right", delta: number) {
    if (side === "left") {
      setLeftWidth((value) => Math.min(340, Math.max(190, value + delta)));
    } else {
      setRightWidth((value) => Math.min(470, Math.max(300, value + delta)));
    }
  }

  const stageStyle = {
    "--split-left": `${leftWidth}px`,
    "--split-right": `${rightWidth}px`,
  } as CSSProperties;

  return (
    <div
      ref={stageRef}
      className={showChapters ? "splitstage splitstage-three" : "splitstage splitstage-two"}
      style={stageStyle}
    >
      {showChapters ? (
        <aside className="pane chapter-rail" aria-label={t("dt.chapters.direct")}>
          <div className="chapter-rail-head">
            <span className="strip-label">{t("dt.chapters.direct")}</span>
            <span className="independent-scroll-label">{t("dt.split.independentScroll")}</span>
          </div>
          <div className="split-chapters-list">
            {chapters.map((chapter, index) => {
              const next = chapters[index + 1];
              const isCurrent =
                currentSec >= (chapter.start_sec ?? 0) &&
                (!next || currentSec < (next.start_sec ?? 0));
              return (
                <button
                  key={`${chapter.start_sec ?? "unknown"}:${chapter.title}:${index}`}
                  type="button"
                  className={`chap-btn${isCurrent ? " active" : ""}`}
                  onClick={() =>
                    chapter.start_sec !== null
                      ? onSeek(formatTimestamp(chapter.start_sec))
                      : undefined
                  }
                >
                  <span className="ts mono">
                    {chapter.start_sec !== null ? formatTimestamp(chapter.start_sec) : "--:--"}
                  </span>
                  <span className="chap-body">
                    <b>{chapter.title}</b>
                    {chapter.summary ? <span className="chap-sum">{chapter.summary}</span> : null}
                  </span>
                </button>
              );
            })}
          </div>
        </aside>
      ) : null}

      {showChapters ? (
        <ResizeHandle
          label={t("dt.split.resize")}
          onPointerDown={(event) => beginResize("left", event)}
          onNudge={(delta) => nudge("left", delta)}
        />
      ) : null}

      <div className="pane pane-center">
        {left}
        {under}
      </div>

      <ResizeHandle
        label={t("dt.split.resize")}
        onPointerDown={(event) => beginResize("right", event)}
        onNudge={(delta) => nudge("right", -delta)}
      />

      <div className="pane pane-right">
        <div className="pane-right-head">
          <Mic size={13} />
          <span className="strip-label">{t("dt.split.transcript")}</span>
          <span className="independent-scroll-label">{t("dt.split.independentScroll")}</span>
        </div>
        {right}
      </div>
    </div>
  );
}

function ResizeHandle({
  label,
  onPointerDown,
  onNudge,
}: {
  label: string;
  onPointerDown: (event: ReactPointerEvent<HTMLDivElement>) => void;
  onNudge: (delta: number) => void;
}) {
  return (
    <div
      className="splitstage-resizer"
      role="separator"
      aria-orientation="vertical"
      aria-label={label}
      tabIndex={0}
      onPointerDown={onPointerDown}
      onKeyDown={(event) => {
        if (event.key === "ArrowLeft") onNudge(-12);
        if (event.key === "ArrowRight") onNudge(12);
      }}
    >
      <GripVertical size={13} />
    </div>
  );
}
