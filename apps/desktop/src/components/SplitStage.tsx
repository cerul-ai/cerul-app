// P1 detail workbench: independently scrollable chapters and transcript with
// two draggable separators around a stable player/citation stage.

import { useEffect, useRef, useState } from "react";
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
  frames?: React.ReactNode;
};

export function SplitStage({
  currentSec,
  chapters,
  onSeek,
  understood,
  left,
  right,
  under,
  frames,
}: SplitStageProps) {
  const t = useT();
  const showChapters = understood && chapters.length > 0;
  const showFrames = Boolean(frames);
  const showNavigation = showChapters || showFrames;
  const stageRef = useRef<HTMLDivElement | null>(null);
  // Percentages let all three panes grow naturally with the window. Dragging
  // adjusts the proportions instead of freezing either rail to a pixel width.
  const [leftWidth, setLeftWidth] = useState(19);
  const [rightWidth, setRightWidth] = useState(28);
  const [navigationTab, setNavigationTab] = useState<"chapters" | "frames">(
    showChapters ? "chapters" : "frames",
  );

  useEffect(() => {
    if (showChapters) setNavigationTab("chapters");
    else if (showFrames) setNavigationTab("frames");
  }, [showChapters, showFrames]);

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
        const next = startLeft + (delta / bounds.width) * 100;
        setLeftWidth(Math.min(28, Math.max(15, Math.min(next, 64 - rightWidth))));
      } else {
        const next = startRight - (delta / bounds.width) * 100;
        setRightWidth(Math.min(36, Math.max(22, Math.min(next, 64 - (showNavigation ? leftWidth : 0)))));
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
    const step = delta < 0 ? -1 : 1;
    if (side === "left") {
      setLeftWidth((value) => Math.min(28, Math.max(15, value + step)));
    } else {
      setRightWidth((value) => Math.min(36, Math.max(22, value + step)));
    }
  }

  const stageStyle = {
    "--split-left": leftWidth,
    "--split-right": rightWidth,
  } as CSSProperties;

  return (
    <div
      ref={stageRef}
      className={showNavigation ? "splitstage splitstage-three" : "splitstage splitstage-two"}
      style={stageStyle}
    >
      {showNavigation ? (
        <aside className="pane chapter-rail" aria-label={t("dt.navigation.title")}>
          <div className="chapter-rail-head">
            <span className="strip-label">{t("dt.navigation.title")}</span>
            <span className="independent-scroll-label">{t("dt.split.independentScroll")}</span>
          </div>
          <div className="detail-navigation-tabs" role="tablist" aria-label={t("dt.navigation.title")}>
            {showChapters ? <button type="button" role="tab" aria-selected={navigationTab === "chapters"} className={navigationTab === "chapters" ? "active" : ""} onClick={() => setNavigationTab("chapters")}>{t("dt.navigation.chapters")}</button> : null}
            {showFrames ? <button type="button" role="tab" aria-selected={navigationTab === "frames"} className={navigationTab === "frames" ? "active" : ""} onClick={() => setNavigationTab("frames")}>{t("dt.navigation.frames")}</button> : null}
          </div>
          <div className="detail-navigation-content">
            {navigationTab === "frames" && showFrames ? frames : (
              <div className="split-chapters-list">
                {chapters.map((chapter, index) => {
                  const next = chapters[index + 1];
                  const isCurrent = currentSec >= (chapter.start_sec ?? 0) && (!next || currentSec < (next.start_sec ?? 0));
                  return (
                    <button key={`${chapter.start_sec ?? "unknown"}:${chapter.title}:${index}`} type="button" className={`chap-btn${isCurrent ? " active" : ""}`} onClick={() => chapter.start_sec !== null ? onSeek(formatTimestamp(chapter.start_sec)) : undefined}>
                      <span className="ts mono">{chapter.start_sec !== null ? formatTimestamp(chapter.start_sec) : "--:--"}</span>
                      <span className="chap-body"><b>{chapter.title}</b>{chapter.summary ? <span className="chap-sum">{chapter.summary}</span> : null}</span>
                    </button>
                  );
                })}
              </div>
            )}
          </div>
        </aside>
      ) : null}

      {showNavigation ? (
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
