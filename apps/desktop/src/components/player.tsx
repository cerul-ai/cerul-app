// Custom video player chrome (migrated from design/cerul-redesign · Area 2).
//
// Replaces the raw HTML5 <video controls> with on-brand chrome: play/pause, a
// steel scrubber with transcript-segment markers (hover preview +
// click-to-seek), time, volume, and fullscreen. The dark chrome is intentional
// and theme-independent, matching how video players conventionally look.
//
// The parent owns the <video> ref so its existing seek / autoplay / keyboard
// effects keep working untouched; this component renders that <video> (sans
// native controls) and mirrors its state via media events for the UI.

import { memo, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Maximize2, Pause, Play, Volume2, VolumeX } from "lucide-react";
import { useT } from "../lib/i18n";

export type PlayerMarker = {
  seconds: number;
  label: string;
  text?: string;
  match?: boolean;
};

export type PlayerChapter = {
  seconds: number;
  title: string;
};

function fmtClock(seconds: number): string {
  const s = Number.isFinite(seconds) && seconds > 0 ? Math.floor(seconds) : 0;
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const sec = String(s % 60).padStart(2, "0");
  return h > 0 ? `${h}:${String(m).padStart(2, "0")}:${sec}` : `${m}:${sec}`;
}

export function CerulPlayer({
  videoRef,
  src,
  markers = [],
  chapters = [],
  ariaLabel,
  onPlay,
  onPause,
  onSeekMarker,
}: {
  videoRef: React.RefObject<HTMLVideoElement | null>;
  src: string;
  markers?: PlayerMarker[];
  chapters?: PlayerChapter[];
  ariaLabel?: string;
  onPlay?: () => void;
  onPause?: () => void;
  onSeekMarker?: (marker: PlayerMarker) => void;
}) {
  const t = useT();
  const containerRef = useRef<HTMLDivElement | null>(null);
  const trackRef = useRef<HTMLDivElement | null>(null);
  const [time, setTime] = useState(0);
  const [duration, setDuration] = useState(0);
  const [playing, setPlaying] = useState(false);
  const [muted, setMuted] = useState(false);
  const [volume, setVolume] = useState(1);
  const [hover, setHover] = useState<{ left: number; marker: PlayerMarker } | null>(null);
  // Real video aspect (w/h), once known; null → fall back to the CSS 16:9.
  const [videoAspect, setVideoAspect] = useState<number | null>(null);

  // Mirror the <video> element's state into React via its media events.
  useEffect(() => {
    const video = videoRef.current;
    if (!video) {
      return;
    }
    const syncTime = () => setTime(video.currentTime);
    const syncDuration = () => setDuration(Number.isFinite(video.duration) ? video.duration : 0);
    const syncPlay = () => {
      setPlaying(true);
      onPlay?.();
    };
    const syncPause = () => {
      setPlaying(false);
      onPause?.();
    };
    const syncVolume = () => {
      setMuted(video.muted);
      setVolume(video.volume);
    };
    // Adapt the stage to the real video shape so vertical / square / ultrawide
    // sources don't sit as a thin strip in a fixed 16:9 box. Clamped so an
    // extreme ratio can't blow up the layout; object-fit:contain still
    // letterboxes anything outside the clamp (never stretches).
    const syncAspect = () => {
      if (video.videoWidth > 0 && video.videoHeight > 0) {
        const ratio = video.videoWidth / video.videoHeight;
        setVideoAspect(Math.min(2.5, Math.max(0.5, ratio)));
      }
    };
    syncDuration();
    syncVolume();
    syncAspect();
    setPlaying(!video.paused);
    setTime(video.currentTime);
    video.addEventListener("timeupdate", syncTime);
    video.addEventListener("durationchange", syncDuration);
    video.addEventListener("loadedmetadata", syncDuration);
    video.addEventListener("loadedmetadata", syncAspect);
    video.addEventListener("resize", syncAspect);
    video.addEventListener("play", syncPlay);
    video.addEventListener("pause", syncPause);
    video.addEventListener("volumechange", syncVolume);
    return () => {
      video.removeEventListener("timeupdate", syncTime);
      video.removeEventListener("durationchange", syncDuration);
      video.removeEventListener("loadedmetadata", syncDuration);
      video.removeEventListener("loadedmetadata", syncAspect);
      video.removeEventListener("resize", syncAspect);
      video.removeEventListener("play", syncPlay);
      video.removeEventListener("pause", syncPause);
      video.removeEventListener("volumechange", syncVolume);
    };
  }, [videoRef, src, onPlay, onPause]);

  const pct = duration > 0 ? (time / duration) * 100 : 0;

  // Chapter starts → contiguous segments over the track. A leading untitled
  // segment covers media that begins before the first chapter.
  const segments = useMemo(() => {
    if (!(duration > 0) || chapters.length === 0) {
      return [];
    }
    const sorted = chapters
      .filter((chapter) => chapter.seconds >= 0 && chapter.seconds < duration)
      .sort((a, b) => a.seconds - b.seconds);
    if (sorted.length === 0) {
      return [];
    }
    const withLead = sorted[0].seconds > 1 ? [{ seconds: 0, title: "" }, ...sorted] : sorted;
    return withLead.map((chapter, index) => ({
      title: chapter.title,
      start: chapter.seconds,
      end: index + 1 < withLead.length ? withLead[index + 1].seconds : duration,
    }));
  }, [chapters, duration]);
  const hasChapters = segments.length > 1;

  const togglePlay = () => {
    const video = videoRef.current;
    if (!video) return;
    if (video.paused) {
      void video.play().catch(() => undefined);
    } else {
      video.pause();
    }
  };

  const seekToClientX = (clientX: number) => {
    const video = videoRef.current;
    const track = trackRef.current;
    if (!video || !track || !(duration > 0)) return;
    const rect = track.getBoundingClientRect();
    const ratio = Math.min(1, Math.max(0, (clientX - rect.left) / rect.width));
    video.currentTime = ratio * duration;
  };
  const onTrackDown = (event: React.PointerEvent) => {
    seekToClientX(event.clientX);
    const move = (ev: PointerEvent) => seekToClientX(ev.clientX);
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  };
  // Sorted copy so hover lookup is a binary search instead of scanning
  // every marker on each mousemove.
  const sortedMarkers = useMemo(
    () => [...markers].sort((a, b) => a.seconds - b.seconds),
    [markers],
  );
  const onTrackMove = (event: React.MouseEvent) => {
    const track = trackRef.current;
    if (!track || !(duration > 0) || (markers.length === 0 && !hasChapters)) {
      return;
    }
    const rect = track.getBoundingClientRect();
    const ratio = (event.clientX - rect.left) / rect.width;
    const target = ratio * duration;
    let lo = 0;
    let hi = sortedMarkers.length - 1;
    while (lo < hi) {
      const mid = (lo + hi) >> 1;
      if (sortedMarkers[mid].seconds < target) lo = mid + 1;
      else hi = mid;
    }
    let nearest: PlayerMarker | null = null;
    let best = 0.025;
    for (const candidate of [sortedMarkers[lo - 1], sortedMarkers[lo]]) {
      if (!candidate) continue;
      const distance = Math.abs(candidate.seconds / duration - ratio);
      if (distance < best) {
        best = distance;
        nearest = candidate;
      }
    }
    if (!nearest && hasChapters) {
      const seconds = ratio * duration;
      const segment = segments.find((entry) => seconds >= entry.start && seconds < entry.end);
      if (segment?.title) {
        setHover({
          left: Math.min(100, Math.max(0, ratio * 100)),
          marker: { seconds: segment.start, label: segment.title },
        });
        return;
      }
      setHover(null);
      return;
    }
    setHover(nearest ? { left: (nearest.seconds / duration) * 100, marker: nearest } : null);
  };

  const seekBy = (deltaSeconds: number) => {
    const video = videoRef.current;
    if (!video || !(duration > 0)) return;
    video.currentTime = Math.min(duration, Math.max(0, video.currentTime + deltaSeconds));
  };
  const onTrackKeyDown = (event: React.KeyboardEvent) => {
    if (event.key === "ArrowRight") {
      event.preventDefault();
      seekBy(5);
    } else if (event.key === "ArrowLeft") {
      event.preventDefault();
      seekBy(-5);
    } else if (event.key === "Home") {
      event.preventDefault();
      seekBy(Number.NEGATIVE_INFINITY);
    } else if (event.key === "End") {
      event.preventDefault();
      seekBy(Number.POSITIVE_INFINITY);
    }
  };
  const adjustVolume = (delta: number) => {
    const video = videoRef.current;
    if (!video) return;
    const next = Math.min(1, Math.max(0, video.volume + delta));
    video.volume = next;
    video.muted = next === 0;
  };
  const onVolKeyDown = (event: React.KeyboardEvent) => {
    if (event.key === "ArrowRight" || event.key === "ArrowUp") {
      event.preventDefault();
      adjustVolume(0.1);
    } else if (event.key === "ArrowLeft" || event.key === "ArrowDown") {
      event.preventDefault();
      adjustVolume(-0.1);
    }
  };

  const onMarkerClick = useCallback(
    (marker: PlayerMarker) => {
      if (onSeekMarker) {
        onSeekMarker(marker);
        return;
      }
      const video = videoRef.current;
      if (video) {
        video.currentTime = marker.seconds;
      }
    },
    [onSeekMarker, videoRef],
  );

  const toggleMute = () => {
    const video = videoRef.current;
    if (video) {
      video.muted = !video.muted;
    }
  };
  const setVolumeFromClientX = (clientX: number, track: HTMLDivElement) => {
    const video = videoRef.current;
    if (!video) return;
    const rect = track.getBoundingClientRect();
    const ratio = Math.min(1, Math.max(0, (clientX - rect.left) / rect.width));
    video.volume = ratio;
    video.muted = ratio === 0;
  };
  const onVolDown = (event: React.PointerEvent<HTMLDivElement>) => {
    const track = event.currentTarget;
    setVolumeFromClientX(event.clientX, track);
    const move = (ev: PointerEvent) => setVolumeFromClientX(ev.clientX, track);
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  };

  const toggleFullscreen = () => {
    const el = containerRef.current;
    if (!el) return;
    if (document.fullscreenElement) {
      void document.exitFullscreen().catch(() => undefined);
    } else {
      void el.requestFullscreen?.().catch(() => undefined);
    }
  };

  const showMarkers = duration > 0 && markers.length > 0;
  const isMuted = muted || volume === 0;

  // Landscape/square fills the column width; portrait is sized by height and
  // centered so it doesn't become a thin strip in a wide box. Until the real
  // ratio is known we keep the CSS 16:9 default.
  const stageStyle: React.CSSProperties | undefined =
    videoAspect === null
      ? undefined
      : videoAspect >= 1
        ? { aspectRatio: String(videoAspect) }
        : { aspectRatio: String(videoAspect), width: "auto", height: "min(70vh, 720px)", marginInline: "auto" };

  return (
    <div className="cplayer" ref={containerRef}>
      <div className="cplayer-stage" onClick={togglePlay} style={stageStyle}>
        {/* eslint-disable-next-line jsx-a11y/media-has-caption */}
        <video ref={videoRef} className="cplayer-video" playsInline src={src} aria-label={ariaLabel} />
        {!playing ? (
          <button
            className="cplayer-bigplay"
            type="button"
            aria-label={t("detail.player.playAria")}
            onClick={(event) => {
              event.stopPropagation();
              togglePlay();
            }}
          >
            <Play size={28} fill="currentColor" />
          </button>
        ) : null}
      </div>

      <div className="cplayer-bar" onClick={(event) => event.stopPropagation()}>
        <div className="cplayer-scrub" onMouseMove={onTrackMove} onMouseLeave={() => setHover(null)}>
          {hover ? (
            <div className="cplayer-tip" style={{ left: `${hover.left}%` }}>
              <span className="cplayer-tip-t mono">{fmtClock(hover.marker.seconds)}</span>
              {hover.marker.text ? `${hover.marker.text.slice(0, 46)}…` : hover.marker.label}
            </div>
          ) : null}
          <div
            className={hasChapters ? "cplayer-track has-chapters" : "cplayer-track"}
            ref={trackRef}
            onPointerDown={onTrackDown}
            role="slider"
            tabIndex={0}
            aria-label={ariaLabel}
            aria-valuemin={0}
            aria-valuemax={Math.round(duration)}
            aria-valuenow={Math.round(time)}
            aria-valuetext={fmtClock(time)}
            onKeyDown={onTrackKeyDown}
          >
            {hasChapters ? (
              segments.map((segment) => (
                <div
                  key={segment.start}
                  className="cplayer-seg"
                  style={{ flexGrow: Math.max(segment.end - segment.start, 1) }}
                >
                  <div
                    className="cplayer-seg-fill"
                    style={{
                      width: `${Math.min(100, Math.max(0, ((time - segment.start) / Math.max(segment.end - segment.start, 0.01)) * 100))}%`,
                    }}
                  />
                </div>
              ))
            ) : (
              <div className="cplayer-fill" style={{ width: `${pct}%` }} />
            )}
            {showMarkers ? (
              <MarkerLayer markers={markers} duration={duration} onMarkerClick={onMarkerClick} />
            ) : null}
            <div className="cplayer-knob" style={{ left: `${pct}%` }} />
          </div>
        </div>

        <div className="cplayer-row">
          <button
            className="cplayer-btn"
            type="button"
            onClick={togglePlay}
            aria-label={playing ? t("detail.player.pauseAria") : t("detail.player.playAria")}
          >
            {playing ? <Pause size={17} fill="currentColor" /> : <Play size={17} fill="currentColor" />}
          </button>
          <span className="cplayer-time mono">
            {fmtClock(time)} <span className="faint">/ {fmtClock(duration)}</span>
          </span>
          <div className="cplayer-grow" />
          <div className="cplayer-vol">
            <button
              className="cplayer-btn"
              type="button"
              onClick={toggleMute}
              aria-label={isMuted ? t("player.unmute") : t("player.mute")}
            >
              {isMuted ? <VolumeX size={17} /> : <Volume2 size={17} />}
            </button>
            <div
              className="cplayer-voltrack"
              onPointerDown={onVolDown}
              role="slider"
              tabIndex={0}
              aria-valuemin={0}
              aria-valuemax={100}
              aria-valuenow={Math.round((isMuted ? 0 : volume) * 100)}
              onKeyDown={onVolKeyDown}
            >
              <div className="cplayer-volfill" style={{ width: `${isMuted ? 0 : volume * 100}%` }} />
            </div>
          </div>
          <button
            className="cplayer-btn"
            type="button"
            onClick={toggleFullscreen}
            aria-label={t("player.fullscreen")}
          >
            <Maximize2 size={16} />
          </button>
        </div>
      </div>
    </div>
  );
}

// Isolated marker buttons: timeupdate fires ~4x/second and re-renders the
// player, but the (potentially thousands of) marker nodes only depend on the
// transcript and duration, so they are memoized out of that hot path.
const MarkerLayer = memo(function MarkerLayer({
  markers,
  duration,
  onMarkerClick,
}: {
  markers: PlayerMarker[];
  duration: number;
  onMarkerClick: (marker: PlayerMarker) => void;
}) {
  return (
    <>
      {markers.map((marker, index) => (
        <button
          key={`${marker.seconds}-${index}`}
          type="button"
          className={marker.match ? "cplayer-mark match" : "cplayer-mark"}
          style={{ left: `${(marker.seconds / duration) * 100}%` }}
          aria-label={marker.label}
          onClick={(event) => {
            event.stopPropagation();
            onMarkerClick(marker);
          }}
        />
      ))}
    </>
  );
});
