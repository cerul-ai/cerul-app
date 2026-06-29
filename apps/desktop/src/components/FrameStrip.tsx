// B-style "关键帧 · 拖动浏览整段" filmstrip for the ItemDetail redesign.
//
// Renders the sampled keyframes (one per VideoUnderstandingEvent, using the
// event's nearest ChunkRecord.frame_path for the thumbnail URL) as a
// horizontal scrub strip, with markers for chapters (dashed underline) and
// key moments (yellow ticks), plus the current seek playhead. Hovering a
// frame reveals that event's `caption` + `visual`; clicking seeks to that
// time via `onSeek`.
//
// When `understood` is false (no understanding for this item), the strip
// still renders whatever keyframes we can derive from the raw sampled
// chunks (FFmpeg samples keyframes regardless of the analysis pass) so users can
// still drag-scrub the whole clip; captions are replaced by a generic
// hover hint.

import { useState } from "react";
import { Image } from "lucide-react";
import * as api from "../lib/api";
import { useT } from "../lib/i18n";

type FrameStripProps = {
  events: api.VideoUnderstandingEvent[];
  chapters: api.VideoUnderstandingChapter[];
  chunks: api.ChunkRecord[];
  durationSec: number | null | undefined;
  currentTime?: number; // seconds, used for the playhead
  understood: boolean;
  onSeek?: (timestamp: string) => void;
};

type Frame = {
  seconds: number;
  label: string;
  url: string | null;
  caption: string | null;
  visual: string | null;
  isHi: boolean;
};

function formatTs(seconds: number): string {
  const s = Number.isFinite(seconds) && seconds > 0 ? Math.floor(seconds) : 0;
  const m = Math.floor(s / 60);
  const sec = String(s % 60).padStart(2, "0");
  return m >= 60
    ? `${Math.floor(m / 60)}:${String(m % 60).padStart(2, "0")}:${sec}`
    : `${m}:${sec}`;
}

// Pick a representative frame for the strip at time t.
// 1. prefer the event's own nearest chunk (if this frame came from an event);
// 2. otherwise the chunk whose [start,end] covers t;
// 3. otherwise the closest chunk by start_sec.
function frameForTime(
  t: number,
  chunks: api.ChunkRecord[],
): { chunk: api.ChunkRecord | null; url: string | null } {
  const frameChunks = chunks.filter((c) => c.frame_path && c.start_sec !== null);
  if (frameChunks.length === 0) {
    return { chunk: null, url: null };
  }
  const inside = frameChunks.find(
    (c) =>
      c.start_sec !== null && c.end_sec !== null &&
      t >= (c.start_sec as number) &&
      t < (c.end_sec as number),
  );
  const pick =
    inside ??
    frameChunks.reduce<api.ChunkRecord | null>(
      (best, c) =>
        best === null
          ? c
          : Math.abs((c.start_sec as number) - t) <
            Math.abs((best.start_sec as number) - t)
            ? c
            : best,
      null,
    );
  const url = pick ? api.chunkFrameUrl(pick.id) : null;
  return { chunk: pick ?? null, url };
}

function sampleEvenly<T>(items: T[], max: number): T[] {
  if (items.length <= max) {
    return items;
  }
  if (max <= 1) {
    return [items[0]];
  }
  const selected: T[] = [];
  let lastIndex = -1;
  for (let i = 0; i < max; i += 1) {
    const index = Math.round((i * (items.length - 1)) / (max - 1));
    if (index !== lastIndex) {
      selected.push(items[index]);
      lastIndex = index;
    }
  }
  return selected;
}

export function FrameStrip({
  events,
  chapters,
  chunks,
  durationSec,
  currentTime = 0,
  understood,
  onSeek,
}: FrameStripProps) {
  const t = useT();
  const total = durationSec && durationSec > 0 ? durationSec : null;
  const [hover, setHover] = useState<number | null>(null);

  // Build the rows of frames. Source order:
  //  - if understood and we have events: one frame per event (those are the
  //    moments analysis marked as key);
  //  - else: sample frames from the chunks themselves (one per chunk that
  //    has a frame) up to a cap, so the strip is still usable.
  let frames: Frame[] = [];
  if (understood && events.length > 0) {
    frames = events
      .filter((e) => e.start_sec !== null && Number.isFinite(e.start_sec))
      .map((e) => {
        const sec = e.start_sec as number;
        const { url } = frameForTime(sec, chunks);
        const visual = e.visual?.trim() ? e.visual.trim() : null;
        return {
          seconds: sec,
          label: formatTs(sec),
          url,
          caption: e.caption?.trim() ? e.caption.trim() : null,
          visual,
          isHi: true,
        };
      });
  } else {
    frames = sampleEvenly(
      chunks
        .filter((c) => c.frame_path && c.start_sec !== null)
        .sort((a, b) => (a.start_sec as number) - (b.start_sec as number)),
      16,
    ).map((c) => {
      const sec = c.start_sec as number;
      return {
        seconds: sec,
        label: formatTs(sec),
        url: api.chunkFrameUrl(c.id),
        caption: null,
        visual: null,
        isHi: false,
      };
    });
  }
  // sort by time for the strip
  frames.sort((a, b) => a.seconds - b.seconds);

  // bounds for the playhead / marks
  const maxSec =
    total ??
    Math.max(
      ...frames.map((f) => f.seconds),
      1,
    );

  const hovered = hover != null ? frames[hover] : null;
  const showLegend =
    understood &&
    (chapters.some((chapter) => chapter.start_sec !== null) ||
      frames.some((frame) => frame.isHi) ||
      currentTime > 0);

  // Empty strip protection: if no frames at all, render nothing so we never
  // leave a blank band.
  if (frames.length === 0) {
    return null;
  }

  return (
    <div className="stripwrap">
      <div className="strip-head">
        <Image size={14} />
        <span className="strip-label">{t("dt.frames.title")}</span>
        {showLegend ? (
          <span className="strip-legend">
            {chapters.some((chapter) => chapter.start_sec !== null) ? (
              <span>
                <i className="strip-legend-chapter" />
                {t("dt.frames.legend.chapter")}
              </span>
            ) : null}
            {frames.some((frame) => frame.isHi) ? (
              <span>
                <i className="strip-legend-hi" />
                {t("dt.frames.legend.hi")}
              </span>
            ) : null}
            {currentTime > 0 ? (
              <span>
                <i className="strip-legend-here" />
                {t("dt.frames.legend.here")}
              </span>
            ) : null}
          </span>
        ) : null}
        <span className="strip-spacer" />
        <span className="faint strip-count">
          {t("dt.frames.total", { n: frames.length })}
        </span>
      </div>

      <div className="strip-track" role="list">
        {frames.map((frame, i) => {
          return (
            <button
              key={`${frame.seconds}-${i}`}
              type="button"
              role="listitem"
              className="strip-frame"
              style={{ width: `${Math.max(100 / frames.length, 8)}%` }}
              onMouseEnter={() => setHover(i)}
              onMouseLeave={() => setHover(null)}
              onFocus={() => setHover(i)}
              onBlur={() => setHover(null)}
              onClick={() => onSeek?.(frame.label)}
              aria-label={`${frame.label}${
                frame.caption ? ` · ${frame.caption}` : ""
              }`}
            >
              {frame.url ? (
                <img src={frame.url} alt="" loading="lazy" draggable="false" />
              ) : (
                <span className="strip-frame-placeholder" />
              )}
              <span className="strip-frame-ts mono">{frame.label}</span>
            </button>
          );
        })}
      </div>

      <div className="strip-rule" aria-hidden="true">
        {chapters
          .filter((c) => c.start_sec !== null)
          .map((c, i) => (
            <i
              key={i}
              className="strip-rule-chapter"
              style={{ left: `${((c.start_sec as number) / maxSec) * 100}%` }}
            />
          ))}
        {frames
          .filter((f) => f.isHi)
          .map((f, i) => (
            <i
              key={i}
              className="strip-rule-hi"
              style={{ left: `${(f.seconds / maxSec) * 100}%` }}
            />
          ))}
        <i
          className="strip-rule-here"
          style={{
            left: `${Math.min(Math.max(currentTime / maxSec, 0), 1) * 100}%`,
          }}
        />
      </div>

      <div className={`strip-tip ${hovered ? "" : "empty"}`}>
        {!hovered ? (
          <span className="faint">
            {understood
              ? t("dt.frames.hoverHint")
              : t("dt.frames.hoverHint.noAnalysis")}
          </span>
        ) : hovered.caption ? (
          <div className="strip-tip-body">
            {hovered.url ? (
              <img src={hovered.url} alt="" className="strip-tip-thumb" />
            ) : null}
            <div className="strip-tip-text">
              <div className="strip-tip-caption">{hovered.caption}</div>
              {hovered.visual ? (
                <div className="faint strip-tip-meta">
                  {t("dt.frames.visualPrefix", { visual: hovered.visual })}
                </div>
              ) : null}
              <div className="faint mono strip-tip-meta">
                {hovered.label}
              </div>
            </div>
          </div>
        ) : (
          <div className="strip-tip-body">
            {hovered.url ? (
              <img src={hovered.url} alt="" className="strip-tip-thumb" />
            ) : null}
            <div className="strip-tip-text">
              <div className="strip-tip-caption faint">
                {understood ? t("dt.frames.notMarked") : t("dt.frames.plainFrame")}
              </div>
              <div className="faint mono strip-tip-meta">{hovered.label}</div>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
