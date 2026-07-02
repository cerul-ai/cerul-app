import { useEffect, useRef, useState } from "react";
import type { KeyboardEvent, MouseEvent } from "react";
import * as api from "./lib/api";
import { classifyWebVideoUrl } from "./lib/validation";
import { cleanMediaTitle, compactPathParent, errorMessage } from "./lib/formatters";
import { useI18n } from "./lib/i18n";
import type { TFunction } from "./lib/i18n";
import { resolveThemePreference, settingString } from "./lib/settings-helpers";
import { invokeHostCommand } from "./lib/desktopHost";
import { loadPersistedUiState, persistFirstRunActive } from "./lib/uiStore";
import { isBackendFallbackSnippet } from "./lib/results";
import {
  OverlayHint,
  OverlayLoading,
  OverlayMark,
  OverlayThumbGlyph,
  OverlayWatermark,
  highlightOverlay,
} from "./components/overlay-leaf";
import { readOverlayRecentSearches } from "./lib/overlay-recent-searches";

type OverlayResult = {
  itemId: string;
  playbackChunkId: string;
  title: string;
  source: string;
  timestamp: string;
  snippet: string;
  contentType: string;
  chunkType: string;
  sourceType: string | null;
  thumbnailUrl: string | null;
};

type SearchState = "idle" | "loading" | "ready" | "error";
type OverlayMode = "search" | "ask";

const DEFAULT_WEB_VIDEO_AUTHOR_MAX = 50;
const searchDebounceMs = 100;
const overlayRetainQueryMs = 30_000;
const defaultHotkeyLabel = "Alt Space";

// F3 · A pasted link (rather than a search phrase) turns the read-only overlay
// into a quick "index this" inbox. Whitespace ⇒ it's a query, not a URL.
function isLikelyUrl(value: string): boolean {
  const v = value.trim();
  if (!v || /\s/.test(v)) {
    return false;
  }
  return /^https?:\/\//i.test(v) || /^(?:www\.)?(?:youtube\.com|youtu\.be)/i.test(v);
}

export function OverlayApp() {
  const { lang, t } = useI18n();
  const [query, setQuery] = useState("");
  const [selectedIndex, setSelectedIndex] = useState(0);
  const [items, setItems] = useState<api.ItemRecord[]>([]);
  const [sources, setSources] = useState<api.SourceRecord[]>([]);
  const [results, setResults] = useState<OverlayResult[]>([]);
  const [searchState, setSearchState] = useState<SearchState>("idle");
  const [mode, setMode] = useState<OverlayMode>("search");
  const [askState, setAskState] = useState<SearchState>("idle");
  const [askAnswer, setAskAnswer] = useState<api.AskResponse | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [askError, setAskError] = useState<string | null>(null);
  const [hotkeyLabel, setHotkeyLabel] = useState(defaultHotkeyLabel);
  const [recentSearches, setRecentSearches] = useState<string[]>(() => readOverlayRecentSearches());
  const [urlQueue, setUrlQueue] = useState<{
    status: "idle" | "queuing" | "done" | "error";
    message?: string;
  }>({ status: "idle" });
  const retainedQueryTimerRef = useRef<number | null>(null);
  const panelRef = useRef<HTMLElement>(null);
  // A successful overlay search/ask is a real "first search" — clear the shared
  // first-run flag so the main window's guidance won't reappear. The overlay is
  // long-lived, so read the flag fresh each time (storeGet is an IPC read of the
  // single main-process store) rather than caching a once-per-session guard:
  // that way a re-run onboarding after an earlier search is still handled, and
  // established users (flag already false) incur no write.
  function clearFirstRunGuidance() {
    loadPersistedUiState()
      .then((state) => {
        if (state.firstRunActive) {
          void persistFirstRunActive(false);
        }
      })
      .catch(() => undefined);
  }
  const trimmedQuery = query.trim();
  const selectedResult = results[selectedIndex];
  const isUrlQuery = mode === "search" && isLikelyUrl(trimmedQuery);

  // Reset the queue affordance whenever the typed/pasted text changes.
  useEffect(() => {
    setUrlQueue({ status: "idle" });
  }, [trimmedQuery]);

  useEffect(() => {
    document.body.classList.add("overlay-body");
    // The native overlay window enables macOS vibrancy; reveal that frosted
    // material by going translucent only there (see extensions.css).
    if (navigator.userAgent.includes("Macintosh")) {
      document.documentElement.dataset.vibrancy = "on";
    }
    return () => document.body.classList.remove("overlay-body");
  }, []);

  // Grow/shrink the native overlay window to fit the panel, so there is no
  // transparent dead-zone below it showing the app underneath.
  useEffect(() => {
    const panel = panelRef.current;
    if (!panel || typeof ResizeObserver === "undefined") {
      return;
    }
    let frame = 0;
    const sync = () => {
      const height = Math.ceil(panel.getBoundingClientRect().height); // window hugs the panel
      void invokeHostCommand("resize_overlay", { height }).catch(() => undefined);
    };
    const observer = new ResizeObserver(() => {
      window.cancelAnimationFrame(frame);
      frame = window.requestAnimationFrame(sync);
    });
    observer.observe(panel);
    sync();
    return () => {
      observer.disconnect();
      window.cancelAnimationFrame(frame);
    };
  }, []);

  useEffect(() => {
    let cancelled = false;

    api.listItems()
      .then((records) => {
        if (!cancelled) {
          setItems(records);
        }
      })
      .catch(() => undefined);
    api.listSources()
      .then((records) => {
        if (!cancelled) {
          setSources(records);
        }
      })
      .catch(() => undefined);
    api.listSettings()
      .then((settings) => {
        if (!cancelled) {
          setHotkeyLabel(formatHotkeyLabel(settings.global_hotkey));
          const prefersLight =
            window.matchMedia?.("(prefers-color-scheme: light)").matches ?? false;
          document.documentElement.dataset.theme = resolveThemePreference(
            settingString(settings, "theme", "Dark"),
            prefersLight,
          );
        }
      })
      .catch(() => undefined);

    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    window.addEventListener("focus", clearRetainedQueryTimer);
    window.addEventListener("storage", refreshRecentSearches);
    return () => {
      window.removeEventListener("focus", clearRetainedQueryTimer);
      window.removeEventListener("storage", refreshRecentSearches);
      clearRetainedQueryTimer();
    };
  }, []);

  function refreshRecentSearches() {
    setRecentSearches(readOverlayRecentSearches());
  }

  useEffect(() => {
    setSelectedIndex(0);
  }, [query]);

  useEffect(() => {
    setSelectedIndex(0);
  }, [mode]);

  useEffect(() => {
    setSelectedIndex((index) => Math.min(index, Math.max(results.length - 1, 0)));
  }, [results.length]);

  useEffect(() => {
    let cancelled = false;

    if (mode !== "search" || isUrlQuery || !trimmedQuery) {
      setSearchState("idle");
      setError(null);
      setResults([]);
      return () => {
        cancelled = true;
      };
    }

    const timer = window.setTimeout(() => {
      setSearchState("loading");
      setError(null);

      api.search(trimmedQuery, 6)
        .then((response) => {
          if (cancelled) {
            return;
          }
          setResults(response.results.map((record) => mapOverlayResult(record, items, sources, t)));
          setSearchState("ready");
          // Only a search that actually matched indexed content counts as the
          // first search — /search can return an empty set before any chunk is
          // indexed, which must not dismiss guidance during the ② takeover.
          if (response.results.length > 0) {
            clearFirstRunGuidance();
          }
        })
        .catch((searchError) => {
          if (cancelled) {
            return;
          }
          setResults([]);
          setError(errorMessage(searchError));
          setSearchState("error");
        });
    }, searchDebounceMs);

    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [trimmedQuery, items, sources, mode, isUrlQuery]);

  useEffect(() => {
    let cancelled = false;

    if (mode !== "ask" || !trimmedQuery) {
      setAskState("idle");
      setAskError(null);
      setAskAnswer(null);
      return () => {
        cancelled = true;
      };
    }

    const timer = window.setTimeout(() => {
      setAskState("loading");
      setAskError(null);
      api
        .askLibrary(trimmedQuery, 5, lang)
        .then((answer) => {
          if (cancelled) {
            return;
          }
          setAskAnswer(answer);
          setAskState("ready");
          // Likewise, only a grounded answer (with citations from indexed
          // content) counts — an empty-library answer must not dismiss guidance.
          if (answer.citations.length > 0) {
            clearFirstRunGuidance();
          }
        })
        .catch((askErr) => {
          if (cancelled) {
            return;
          }
          setAskAnswer(null);
          setAskError(errorMessage(askErr));
          setAskState("error");
        });
    }, searchDebounceMs);

    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [trimmedQuery, mode, lang]);

  function clearRetainedQueryTimer() {
    if (retainedQueryTimerRef.current !== null) {
      window.clearTimeout(retainedQueryTimerRef.current);
      retainedQueryTimerRef.current = null;
    }
  }

  function resetOverlayQuery() {
    setQuery("");
    setSelectedIndex(0);
    setResults([]);
    setSearchState("idle");
    setError(null);
    setAskAnswer(null);
    setAskState("idle");
    setAskError(null);
  }

  function scheduleRetainedQueryReset() {
    clearRetainedQueryTimer();
    retainedQueryTimerRef.current = window.setTimeout(() => {
      resetOverlayQuery();
      retainedQueryTimerRef.current = null;
    }, overlayRetainQueryMs);
  }

  async function hideOverlay(retainQuery = false) {
    if (retainQuery && trimmedQuery) {
      scheduleRetainedQueryReset();
    } else {
      clearRetainedQueryTimer();
    }
    await invokeHostCommand("hide_overlay").catch(() => undefined);
  }

  async function openResult(result: OverlayResult) {
    clearRetainedQueryTimer();
    await invokeHostCommand("open_main_result", {
      itemId: result.itemId,
      playbackChunkId: result.playbackChunkId,
      timestamp: result.timestamp,
    }).catch(() => undefined);
    resetOverlayQuery();
  }

  async function copyResultLink(result: OverlayResult) {
    const params = new URLSearchParams({ t: result.timestamp });
    params.set("playbackChunkId", result.playbackChunkId);
    const link = `cerul-app://item/${encodeURIComponent(result.itemId)}?${params.toString()}`;
    await navigator.clipboard?.writeText(link).catch(() => undefined);
  }

  // F3 · Queue a pasted link for indexing, reusing the same source payloads the
  // Add-source dialog uses (YouTube vs. podcast/RSS). No new backend behaviour.
  async function queueUrl() {
    const url = trimmedQuery;
    if (!url) {
      return;
    }
    setUrlQueue({ status: "queuing" });
    try {
      // Same classification as the Add-source dialog: a pasted single video
      // used to be queued as a 50-video channel fetch.
      const classified = classifyWebVideoUrl(url, t);
      if (classified.ok) {
        await api.addSource("web_video", {
          url: classified.url,
          platform: classified.platform,
          source_kind: classified.sourceKind,
          max_videos: classified.sourceKind === "author" ? 20 : 1,
        });
      } else {
        await api.addSource("rss_podcast", { url, max_episodes: 50 });
      }
      setUrlQueue({ status: "done" });
    } catch (err) {
      setUrlQueue({ status: "error", message: errorMessage(err) });
    }
  }

  function handleKeyDown(event: KeyboardEvent<HTMLInputElement>) {
    if (event.key === "Escape") {
      event.preventDefault();
      void hideOverlay(true);
      return;
    }

    if (event.key === "ArrowDown") {
      event.preventDefault();
      if (!results.length) {
        return;
      }
      setSelectedIndex((index) => Math.min(index + 1, results.length - 1));
      return;
    }

    if (event.key === "ArrowUp") {
      event.preventDefault();
      if (!results.length) {
        return;
      }
      setSelectedIndex((index) => Math.max(index - 1, 0));
      return;
    }

    if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "c" && selectedResult) {
      event.preventDefault();
      void copyResultLink(selectedResult);
      return;
    }

    if (event.key === "Enter" && isUrlQuery) {
      event.preventDefault();
      if (urlQueue.status === "idle" || urlQueue.status === "error") {
        void queueUrl();
      }
      return;
    }

    if (event.key === "Enter" && selectedResult) {
      event.preventDefault();
      void openResult(selectedResult);
    }
  }

  function handleBackdropMouseDown(event: MouseEvent<HTMLElement>) {
    if (event.target === event.currentTarget) {
      void hideOverlay(true);
    }
  }

  const overlayState: "empty" | "loading" | "error" | "results" | "noresult" =
    !trimmedQuery
      ? "empty"
      : searchState === "loading"
        ? "loading"
        : searchState === "error"
          ? "error"
          : results.length
            ? "results"
            : searchState === "ready"
              ? "noresult"
              : "loading";
  const askOverlayState: "empty" | "loading" | "error" | "results" | "noresult" =
    !trimmedQuery
      ? "empty"
      : askState === "loading"
        ? "loading"
        : askState === "error"
          ? "error"
          : askAnswer
            ? "results"
            : askState === "ready"
              ? "noresult"
              : "loading";
  const activeOverlayState = mode === "ask" ? askOverlayState : overlayState;

  return (
    <main className="overlay-root" onMouseDown={handleBackdropMouseDown}>
      <section
        ref={panelRef}
        className="overlay-panel"
        data-state={activeOverlayState}
        aria-label={t("overlay.panelAria")}
      >
        <div className="overlay-search">
          <OverlayMark />
          <input
            className="overlay-input"
            autoFocus
            value={query}
            onChange={(event) => {
              clearRetainedQueryTimer();
              setQuery(event.target.value);
            }}
            onKeyDown={handleKeyDown}
            placeholder={t("overlay.searchPlaceholder")}
          />
          <OverlayHint state={activeOverlayState} hotkeyLabel={hotkeyLabel} />
        </div>
        <div className="overlay-tabs" role="tablist" aria-label={t("overlay.tabs.aria")}>
          <button
            type="button"
            className={mode === "search" ? "active" : ""}
            role="tab"
            aria-selected={mode === "search"}
            onClick={() => setMode("search")}
          >
            {t("overlay.tab.search")}
          </button>
          <button
            type="button"
            className={mode === "ask" ? "active" : ""}
            role="tab"
            aria-selected={mode === "ask"}
            onClick={() => setMode("ask")}
          >
            {t("overlay.tab.ask")}
          </button>
          <span>{mode === "ask" ? t("overlay.ask.hint") : t("overlay.url.hint")}</span>
        </div>

        <div className="overlay-panel-body">
          {mode === "ask" ? (
            <>
              {askOverlayState === "empty" ? (
                <div className="overlay-empty">
                  <strong>{t("overlay.ask.emptyTitle")}</strong>
                  <span>{t("overlay.ask.emptyBody")}</span>
                </div>
              ) : null}
              {askOverlayState === "loading" ? <OverlayLoading /> : null}
              {askOverlayState === "error" ? (
                <div className="overlay-error">
                  <strong>{t("overlay.error.title")}</strong>
                  <span>{askError ?? t("overlay.error.fallback")}</span>
                </div>
              ) : null}
              {askOverlayState === "results" && askAnswer ? (
                <div className="overlay-answer">
                  <p>{askAnswer.answer}</p>
                  {askAnswer.citations.length > 0 ? (
                    <div className="overlay-answer-cites">
                      {askAnswer.citations.map((citation) => (
                        <button
                          key={citation.playback_chunk_id}
                          type="button"
                          onClick={() =>
                            void openResult({
                              itemId: citation.item_id,
                              playbackChunkId: citation.playback_chunk_id,
                              title: citation.title,
                              source: citation.title,
                              timestamp: citation.timestamp,
                              snippet: citation.snippet,
                              contentType: "video",
                              chunkType: "transcript",
                              sourceType: null,
                              thumbnailUrl: null,
                            })
                          }
                        >
                          <span className="mono">{citation.timestamp}</span>
                          <strong>{citation.title}</strong>
                          <small>{citation.snippet}</small>
                        </button>
                      ))}
                    </div>
                  ) : null}
                </div>
              ) : null}
            </>
          ) : isUrlQuery ? (
            <div className="overlay-urlrow">
              <span className="overlay-urlrow__plus" aria-hidden="true">+</span>
              <div className="overlay-urlrow__main">
                <span className="overlay-urlrow__title">{t("overlay.url.title")}</span>
                <span className="overlay-urlrow__link">{trimmedQuery}</span>
              </div>
              {urlQueue.status === "done" ? (
                <span className="overlay-urlrow__done">{t("overlay.url.queued")}</span>
              ) : (
                <button
                  type="button"
                  className="overlay-urlrow__btn"
                  disabled={urlQueue.status === "queuing"}
                  onClick={() => void queueUrl()}
                >
                  {urlQueue.status === "queuing" ? t("overlay.url.queuing") : t("overlay.url.queue")}
                </button>
              )}
            </div>
          ) : (
          <>
          {overlayState === "empty" && recentSearches.length > 0 ? (
            <>
              <div className="overlay-glabel">{t("overlay.recents.title")}</div>
              <div className="overlay-recents" aria-label={t("overlay.recentsAria")}>
                {recentSearches.map((recent) => (
                  <button
                    key={recent}
                    type="button"
                    onClick={() => {
                      clearRetainedQueryTimer();
                      setQuery(recent);
                    }}
                  >
                    {recent}
                  </button>
                ))}
              </div>
            </>
          ) : null}

          {overlayState === "empty" && recentSearches.length === 0 ? (
            <div className="overlay-empty">
              <strong>{t("overlay.empty.idleTitle")}</strong>
              <span>{t("overlay.empty.idleNoRecents")}</span>
            </div>
          ) : null}

          {overlayState === "loading" ? <OverlayLoading /> : null}

          {overlayState === "error" ? (
            <div className="overlay-error">
              <strong>{t("overlay.error.title")}</strong>
              <span>{error ?? t("overlay.error.fallback")}</span>
            </div>
          ) : null}

          {overlayState === "results" ? (
            <>
              <div className="overlay-glabel">
                {t("overlay.results.count", { count: results.length })}
              </div>
              <div
                className="overlay-results"
                role="listbox"
                aria-label={t("overlay.resultsAria")}
              >
                {results.map((result, index) => {
                  const modality = overlayModality(result.contentType, result.chunkType, result.sourceType, t);
                  const isPodcast = modality.key === "podcast";
                  return (
                    <button
                      key={result.playbackChunkId}
                      type="button"
                      role="option"
                      aria-selected={index === selectedIndex}
                      className={
                        index === selectedIndex ? "overlay-result active" : "overlay-result"
                      }
                      onMouseEnter={() => setSelectedIndex(index)}
                      onClick={() => void openResult(result)}
                    >
                      <span
                        className="overlay-thumb"
                        data-modality={isPodcast ? "podcast" : "default"}
                      >
                        <OverlayThumbGlyph contentType={result.contentType} chunkType={result.chunkType} />
                        {result.thumbnailUrl ? (
                          <img
                            className="overlay-thumb__img"
                            src={result.thumbnailUrl}
                            alt=""
                            loading="lazy"
                            onError={(event) => {
                              event.currentTarget.style.display = "none";
                            }}
                          />
                        ) : null}
                        <span className="overlay-thumb__tt">{result.timestamp}</span>
                      </span>
                      <span className="overlay-result__main">
                        <span className="overlay-result__title">
                          {overlayMetaLabel(result.title, result.source)}
                        </span>
                        <span className="overlay-result__snippet">
                          {highlightOverlay(result.snippet, query)}
                        </span>
                      </span>
                      <span className="overlay-result__meta">
                        <span className="overlay-result__ts">{result.timestamp}</span>
                        <span className="overlay-result__mod">
                          <span
                            className="overlay-dot"
                            data-modality={isPodcast ? "podcast" : "default"}
                          />
                          {modality.label}
                        </span>
                      </span>
                    </button>
                  );
                })}
              </div>
            </>
          ) : null}

          {overlayState === "noresult" ? (
            <div className="overlay-empty">
              <strong>{t("overlay.empty.noMatchesTitle")}</strong>
              <span>{t("overlay.empty.noMatchesBody")}</span>
              <OverlayWatermark />
            </div>
          ) : null}
          </>
          )}
        </div>
      </section>
    </main>
  );
}

function overlayModality(contentType: string, chunkType: string, sourceType: string | null, t: TFunction) {
  if (contentType === "image" || isVisualChunk(chunkType)) {
    return { key: "visual" as const, label: t("overlay.modality.visual") };
  }
  if (sourceType === "rss_podcast") {
    return { key: "podcast" as const, label: t("overlay.modality.podcast") };
  }
  return { key: "voice" as const, label: t("overlay.modality.voice") };
}

function isVisualChunk(chunkType: string) {
  return chunkType === "keyframe" || chunkType === "image" || chunkType === "ocr" || chunkType === "understanding";
}

function mapOverlayResult(
  record: api.SearchResultRecord,
  items: api.ItemRecord[],
  sources: api.SourceRecord[],
  t: TFunction,
): OverlayResult {
  const item = items.find((candidate) => candidate.id === record.item_id);
  const source = item ? sources.find((candidate) => candidate.id === item.source_id) : undefined;
  const playbackChunkId = record.playback_chunk_id ?? record.chunk_id ?? "";

  // New search results provide a representative frame chunk id. Keep the direct
  // frame_path branch for older local cores during development.
  const thumbnailUrl = record.frame_path
    ? api.chunkFrameUrl(playbackChunkId)
    : record.nearest_frame_chunk_id
      ? api.chunkFrameUrl(record.nearest_frame_chunk_id)
      : item?.thumbnail_chunk_id
        ? api.chunkFrameUrl(item.thumbnail_chunk_id)
        : null;

  return {
    itemId: record.item_id,
    playbackChunkId,
    // Prefer the title the backend joins into the result; the locally-fetched
    // items list can be empty/stale and leave the row showing a raw id.
    title: cleanMediaTitle(
      record.item_title ?? item?.title ?? item?.raw_path ?? item?.external_id ?? record.item_id,
    ),
    source: overlaySourceLabel(item, sources, t),
    timestamp: formatTimestamp(record.start_sec),
    snippet: overlaySnippet(record, t),
    contentType: item?.content_type ?? "video",
    chunkType: record.chunk_type,
    sourceType: source?.type ?? null,
    thumbnailUrl,
  };
}

// The title and the source label often resolve to the same string (e.g. a
// YouTube item whose channel name matches its title, or an un-cleanable raw
// media id used as both). Collapse those so the row never reads "X - X".
function overlayMetaLabel(title: string, source: string): string {
  const a = title.trim();
  const b = source.trim();
  if (!a) return b;
  if (!b) return a;
  const al = a.toLowerCase();
  const bl = b.toLowerCase();
  if (al === bl || al.includes(bl)) return a;
  if (bl.includes(al)) return b;
  return `${a} · ${b}`;
}

function overlaySourceLabel(
  item: api.ItemRecord | undefined,
  sources: api.SourceRecord[],
  t: TFunction,
) {
  if (!item) {
    return t("overlay.status.mediaIndex");
  }

  const metadataLabel =
    metadataString(item.metadata, "channel") ??
    metadataString(item.metadata, "uploader") ??
    metadataString(item.metadata, "playlist") ??
    metadataString(item.metadata, "source");
  if (metadataLabel) {
    return metadataLabel;
  }

  const source = sources.find((candidate) => candidate.id === item.source_id);
  return source ? overlaySourceName(source, t) : compactPathParent(item.raw_path) ?? item.source_id;
}

function overlaySourceName(source: api.SourceRecord, t: TFunction) {
  const namedValue =
    sourceConfigString(source.config, "name") ?? sourceConfigString(source.config, "title");
  if (namedValue) {
    return namedValue;
  }

  if (source.type.startsWith("folder_")) {
    const path = sourceConfigString(source.config, "path");
    return path ? compactPathLabel(path) : t("overlay.source.localFolder");
  }

  if (source.type === "youtube" || source.type === "web_video") {
    const url =
      sourceConfigString(source.config, "channel_url") ?? sourceConfigString(source.config, "url");
    const label =
      source.type === "web_video" ? t("overlay.source.webVideo") : t("overlay.source.youtube");
    return url ? compactUrlLabel(url, label) : label;
  }

  if (source.type === "rss_podcast") {
    const feedUrl =
      sourceConfigString(source.config, "feed_url") ?? sourceConfigString(source.config, "url");
    return feedUrl ? compactUrlLabel(feedUrl, t("overlay.source.podcast")) : t("overlay.source.podcast");
  }

  const fallback =
    sourceConfigString(source.config, "path") ??
    sourceConfigString(source.config, "url") ??
    sourceConfigString(source.config, "feed_url") ??
    sourceConfigString(source.config, "channel_url");
  return fallback ? cleanMediaTitle(fallback) : source.id;
}

function overlaySnippet(record: api.SearchResultRecord, t: TFunction) {
  const snippet = record.snippet.trim();
  const isFallback = isBackendFallbackSnippet(snippet, record.chunk_type, record.start_sec);
  if (snippet && !isFallback && !snippet.includes("/cache/pipeline/") && !snippet.startsWith("/Users/")) {
    return snippet;
  }
  // The thumbnail badge and the meta column already carry this result's
  // timestamp, so the placeholder snippet stays timestamp-free — otherwise a
  // text-less visual row would show the same time three times over.
  if (record.chunk_type === "keyframe" || record.chunk_type === "image" || record.chunk_type === "ocr") {
    return t("overlay.snippet.visualMatch");
  }
  if (record.chunk_type === "understanding") {
    return t("overlay.snippet.understandingMatch");
  }
  return t("overlay.snippet.searchMatch");
}

function sourceConfigString(config: Record<string, unknown>, key: string) {
  const value = config[key];
  return typeof value === "string" && value.trim() ? value.trim() : null;
}

function metadataString(metadata: Record<string, unknown>, key: string) {
  const value = metadata[key];
  return typeof value === "string" && value.trim() ? value.trim() : null;
}

function compactPathLabel(path: string) {
  const normalized = path.replace(/\\/g, "/").replace(/\/+$/, "");
  const segments = normalized.split("/").filter(Boolean);
  return segments.at(-1) ?? path;
}

function compactUrlLabel(value: string, fallback: string) {
  try {
    const url = new URL(value);
    const segments = url.pathname.split("/").filter(Boolean);
    const handle = segments.find((segment) => segment.startsWith("@"));
    const label = handle ?? segments.at(-1) ?? url.hostname.replace(/^www\./, "");
    return label || fallback;
  } catch {
    return value.trim() || fallback;
  }
}

function formatHotkeyLabel(value: unknown) {
  if (typeof value !== "string" || !value.trim()) {
    return defaultHotkeyLabel;
  }

  return value.replace(/\+/g, " ");
}

function formatTimestamp(seconds: number | null) {
  if (seconds === null || seconds < 0) {
    return "00:00";
  }
  const total = Math.round(seconds);
  const minutes = Math.floor(total / 60);
  const remaining = `${total % 60}`.padStart(2, "0");
  return `${minutes}:${remaining}`;
}
