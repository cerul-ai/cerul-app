// Clip-export trim popover (design: A+ · 视频预览 + 拖拽裁剪 + 步进器).
//
// Opens from the detail header "导出片段" button. Resolves the chunk to clip
// from the LIVE video playhead at open time (so the exported segment and its
// filename track where the user is actually watching — not the stale
// currentTimestamp). The clip window is [chunkStart - before, chunkEnd + after];
// before/after are dragged on the trim track or nudged 1s at a time. The
// preview is a real <video> seeked to the edge being adjusted, object-fit:
// contain so any aspect ratio (vertical / square / ultrawide) fits without
// stretching. Backend caps each side at 30s and the total at 120s.

import { useEffect, useRef, useState } from "react";
import { Loader2, Scissors } from "lucide-react";
import * as api from "../lib/api";
import { errorMessage, formatTimestamp, parseTimestampSeconds } from "../lib/formatters";
import { useT } from "../lib/i18n";
import { useClickOutside, useEscapeToClose } from "../lib/use-dismissable";
import type { TranscriptLine } from "../lib/types";

const SIDE = 30; // max seconds per side (matches backend clamp)

/** The chunk to clip, resolved from the live playhead at open time. */
export type ClipTarget = { chunkId: string; startSec: number; endSec: number };

/** Resolve the chunk under the current playhead. Prefers the chunk whose
 * [start,end] contains the time, else the nearest by start. */
export function resolveClipTarget(
  lines: TranscriptLine[],
  currentSec: number,
): ClipTarget | null {
  let nearest: ClipTarget | null = null;
  let nearestDist = Number.POSITIVE_INFINITY;
  for (const line of lines) {
    const start = line.startSec ?? parseTimestampSeconds(line.time);
    if (!Number.isFinite(start)) continue;
    const end = line.endSec && line.endSec > start ? line.endSec : start + 8;
    if (currentSec >= start && currentSec <= end) {
      return { chunkId: line.id, startSec: start, endSec: end };
    }
    const dist = Math.abs(start - currentSec);
    if (dist < nearestDist) {
      nearestDist = dist;
      nearest = { chunkId: line.id, startSec: start, endSec: end };
    }
  }
  return nearest;
}

export function ClipExportButton({
  contentType,
  disabled,
  resolveTarget,
}: {
  contentType: string;
  disabled: boolean;
  /** Read the live playhead and return the chunk to clip (or null). */
  resolveTarget: () => ClipTarget | null;
}) {
  const t = useT();
  const [open, setOpen] = useState(false);
  const [target, setTarget] = useState<ClipTarget | null>(null);
  const wrapRef = useRef<HTMLDivElement | null>(null);
  useEscapeToClose(() => setOpen(false), open);
  useClickOutside(wrapRef, () => setOpen(false), open);

  if (contentType !== "video") {
    return null;
  }

  function toggle() {
    if (open) {
      setOpen(false);
      return;
    }
    const next = resolveTarget();
    if (!next) return;
    setTarget(next);
    setOpen(true);
  }

  return (
    <div className="clip-export-wrap" ref={wrapRef}>
      <button
        className="btn btn-secondary sm"
        type="button"
        disabled={disabled}
        aria-expanded={open}
        onClick={toggle}
      >
        <Scissors size={15} />
        <span>{t("detail.action.exportClip")}</span>
      </button>
      {open && target ? <ClipTrimPanel target={target} onClose={() => setOpen(false)} /> : null}
    </div>
  );
}

function ClipTrimPanel({ target, onClose }: { target: ClipTarget; onClose: () => void }) {
  const t = useT();
  const trackRef = useRef<HTMLDivElement | null>(null);
  const videoRef = useRef<HTMLVideoElement | null>(null);
  const [before, setBefore] = useState(2);
  const [after, setAfter] = useState(2);
  const [edge, setEdge] = useState<"in" | "out">("in");
  const [status, setStatus] = useState<"idle" | "exporting" | "done" | "error">("idle");
  const [message, setMessage] = useState<string | null>(null);

  const chunkStart = Math.max(0, target.startSec);
  const chunkEnd = Math.max(chunkStart + 1, target.endSec);
  const w0 = Math.max(0, chunkStart - SIDE);
  const w1 = chunkEnd + SIDE;
  const inT = Math.max(w0, chunkStart - before);
  const outT = Math.min(w1, chunkEnd + after);
  const pct = (v: number) => ((v - w0) / (w1 - w0)) * 100;

  // Seek the preview to whichever edge is being adjusted.
  useEffect(() => {
    const video = videoRef.current;
    if (!video) return;
    const target = edge === "in" ? inT : outT;
    const apply = () => {
      try {
        video.currentTime = target;
      } catch {
        /* not seekable yet */
      }
    };
    if (video.readyState >= 1) apply();
    else video.addEventListener("loadedmetadata", apply, { once: true });
  }, [edge, inT, outT]);

  // Snap to whole seconds (1s precision everywhere — matches the steppers).
  const clamp = (n: number) => Math.max(0, Math.min(SIDE, Math.round(n)));

  function timeAt(clientX: number) {
    const rect = trackRef.current?.getBoundingClientRect();
    if (!rect) return chunkStart;
    const p = Math.min(1, Math.max(0, (clientX - rect.left) / rect.width));
    return w0 + p * (w1 - w0);
  }

  function startDrag(which: "in" | "out") {
    return (event: React.PointerEvent) => {
      event.preventDefault();
      setEdge(which);
      const move = (ev: PointerEvent) => {
        const time = timeAt(ev.clientX);
        if (which === "in") setBefore(clamp(chunkStart - time));
        else setAfter(clamp(time - chunkEnd));
      };
      const up = () => {
        window.removeEventListener("pointermove", move);
        window.removeEventListener("pointerup", up);
      };
      window.addEventListener("pointermove", move);
      window.addEventListener("pointerup", up);
      move(event.nativeEvent);
    };
  }

  function nudge(which: "in" | "out", delta: number) {
    setEdge(which);
    if (which === "in") setBefore((b) => clamp(b + delta));
    else setAfter((a) => clamp(a + delta));
  }

  async function exportClip() {
    setStatus("exporting");
    setMessage(null);
    try {
      const response = await fetch(
        api.videoClipUrl(target.chunkId, { beforeSec: before, afterSec: after }),
      );
      if (!response.ok) {
        throw new Error(t("detail.action.exportFailed", { status: response.status }));
      }
      const blob = await response.blob();
      const url = URL.createObjectURL(blob);
      const anchor = document.createElement("a");
      anchor.href = url;
      anchor.download = `cerul-clip-${formatTimestamp(inT).replace(/:/g, "-")}.mp4`;
      document.body.appendChild(anchor);
      anchor.click();
      anchor.remove();
      window.setTimeout(() => URL.revokeObjectURL(url), 4000);
      setStatus("done");
      window.setTimeout(onClose, 700);
    } catch (error) {
      setStatus("error");
      setMessage(errorMessage(error));
    }
  }

  return (
    <div className="clip-pop" role="dialog" aria-label={t("detail.action.exportClip")}>
      <div className="clip-prev">
        {/* muted preview; object-fit:contain letterboxes any aspect ratio */}
        {/* eslint-disable-next-line jsx-a11y/media-has-caption */}
        <video
          ref={videoRef}
          className="clip-frame"
          src={api.videoSegmentUrl(target.chunkId)}
          muted
          playsInline
          preload="metadata"
        />
        <span className="clip-tlabel mono">
          {edge === "in" ? formatTimestamp(inT) : formatTimestamp(outT)}
        </span>
      </div>

      <div className="clip-track" ref={trackRef}>
        <div
          className="clip-base"
          style={{ left: `${pct(chunkStart)}%`, right: `${100 - pct(chunkEnd)}%` }}
        />
        <div className="clip-sel" style={{ left: `${pct(inT)}%`, right: `${100 - pct(outT)}%` }} />
        <button
          type="button"
          className="clip-handle"
          aria-label={t("detail.clip.before")}
          style={{ left: `calc(${pct(inT)}% - 7px)` }}
          onPointerDown={startDrag("in")}
        />
        <button
          type="button"
          className="clip-handle"
          aria-label={t("detail.clip.after")}
          style={{ left: `calc(${pct(outT)}% - 7px)` }}
          onPointerDown={startDrag("out")}
        />
      </div>

      <div className="clip-read">
        <span className="mono clip-range">
          {formatTimestamp(inT)} <b>→</b> {formatTimestamp(outT)}
        </span>
        <span className="clip-dur">· {formatTimestamp(outT - inT)}</span>
      </div>

      <div className="clip-steps">
        <div className="clip-step">
          <span>{t("detail.clip.before")}</span>
          <span className="clip-ctl">
            <button type="button" onClick={() => nudge("in", -1)} disabled={before <= 0}>
              −
            </button>
            <span className="mono">{before}s</span>
            <button type="button" onClick={() => nudge("in", 1)} disabled={before >= SIDE}>
              +
            </button>
          </span>
        </div>
        <div className="clip-step">
          <span>{t("detail.clip.after")}</span>
          <span className="clip-ctl">
            <button type="button" onClick={() => nudge("out", -1)} disabled={after <= 0}>
              −
            </button>
            <span className="mono">{after}s</span>
            <button type="button" onClick={() => nudge("out", 1)} disabled={after >= SIDE}>
              +
            </button>
          </span>
        </div>
      </div>

      {status === "error" && message ? <p className="clip-err">{message}</p> : null}

      <div className="clip-foot">
        <span className="clip-hint">{t("detail.clip.hint")}</span>
        <button
          className="btn btn-primary sm"
          type="button"
          disabled={status === "exporting"}
          onClick={() => void exportClip()}
        >
          {status === "exporting" ? <Loader2 size={15} className="spin" /> : null}
          <span>
            {status === "exporting"
              ? t("detail.action.exportingClip")
              : status === "done"
                ? t("detail.action.clipExported")
                : t("detail.clip.export")}
          </span>
        </button>
      </div>
    </div>
  );
}
