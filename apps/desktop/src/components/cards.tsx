// Result and library item cards. Extracted from App.tsx (B13 Phase B).
//
// Pure props-driven. ResultCard renders a single ranked search result
// with the matched-text highlight — the most important affordance
// for a search result. ItemCard renders a library entry
// with an optional selection checkbox and an indexing progress
// overlay.

import {
  Check,
  Eye,
  FileAudio,
  FileText,
  FileVideo,
  Image as ImageIcon,
  Mic,
  Play,
  Sparkles,
} from "lucide-react";
import { useState } from "react";
import { useT, type TFunction } from "../lib/i18n";
import {
  itemHasPartialIndex,
  itemKindLabel,
} from "../lib/items";
import { resultModality } from "../lib/results";
import type { Item, Result } from "../lib/types";
import { ProgressBar, highlightSnippet } from "./transcript";

// Single searchability chip summarising an item's state, mirroring the
// redesign baseline (语音 + 画面可搜 / 仅语音可搜 / 索引中 · % / 处理失败).
/** Just the searchable-modality label (speech / visual), independent of index
 * status — for the detail subtitle, where indexed/failed status is shown
 * separately. itemSearchability() below folds status in for library cards. */
export function itemModalityLabel(item: Item, t: TFunction): string {
  if (item.contentType === "document") {
    return t("library.itemCard.searchDocument");
  }
  const hasVisual =
    item.contentType === "image" ||
    (item.contentType === "video" && item.visualIndexStatus === "indexed");
  const hasSpeech =
    (item.contentType === "video" || item.contentType === "audio") && item.hasAudio !== false;
  if (hasVisual && hasSpeech) {
    return t("library.itemCard.searchSpeechVisual");
  }
  if (hasVisual) {
    return t("library.itemCard.searchVisualOnly");
  }
  return t("library.itemCard.searchSpeechOnly");
}

export function ResultModalityIcon({
  result,
  size,
}: {
  result: Result;
  size: number;
}) {
  const modality = resultModality(result);
  if (modality === "audio") {
    return <FileAudio size={size} />;
  }
  if (modality === "image") {
    return <ImageIcon size={size} />;
  }
  if (modality === "document") {
    return <FileText size={size} />;
  }
  return <FileVideo size={size} />;
}

export function ResultCard({
  result,
  index,
  selected,
  expanded,
  onFocus,
  onOpen,
  query,
}: {
  result: Result;
  index: number;
  selected: boolean;
  expanded: boolean;
  onFocus: () => void;
  onOpen: (result: Result) => void;
  query: string;
}) {
  const t = useT();
  const className = [
    "result-card",
    "result-row",
    selected ? "active selected" : "",
    expanded ? "expanded" : "",
  ].filter(Boolean).join(" ");
  const modality = resultModality(result);
  const modalityLabel =
    modality === "audio"
      ? t("result.modality.spoken")
      : modality === "image"
        ? t("result.modality.shown")
        : modality === "document"
          ? t("result.modality.document")
          : t("result.modality.both");
  const ModalityBadgeIcon =
    modality === "audio"
      ? Mic
      : modality === "image"
        ? Eye
        : modality === "document"
          ? FileText
          : Sparkles;

  return (
    <article
      className={className}
      data-result-index={index}
      role="button"
      tabIndex={0}
      aria-selected={selected}
      aria-expanded={result.moreMatches.length > 0 ? expanded : undefined}
      onFocus={onFocus}
      onClick={() => onOpen(result)}
      onKeyDown={(event) => {
        if ((event.key === "Enter" || event.key === " ") && event.target === event.currentTarget) {
          event.preventDefault();
          onOpen(result);
        }
      }}
    >
      <span className={`thumb ${result.thumbnailUrl ? "has-image" : result.color}`}>
        {result.thumbnailUrl ? (
          <img src={result.thumbnailUrl} alt="" loading="lazy" />
        ) : (
          <>
            {modality === "document" ? (
              <FileText size={24} />
            ) : (
              <Play size={24} fill="currentColor" />
            )}
            <small className="mono">{result.timestamp}</small>
          </>
        )}
      </span>
      <span className="result-body">
        <span className="result-meta">
          <span className="chip neutral result-source-label">
            <span className="dot" />
            {result.source}
          </span>
          <em className={`chip modality-pill ${modality}`}>
            <ModalityBadgeIcon size={14} />
            {modalityLabel}
          </em>
          <em className="chip score-pill mono" title={result.scoreTitle}>
            {result.scoreLabel}
          </em>
        </span>
        <blockquote className="result-quote clamp3">
          {highlightSnippet(result.snippet, query)}
        </blockquote>
        <span className="result-citation-line">
          <strong className="clamp1">{result.title}</strong>
          <span className="mono">{result.timestamp}</span>
          {result.duration ? <span className="muted">/ {result.duration}</span> : null}
        </span>
        {result.moreMatches.length > 0 && !expanded ? (
          <span className="result-more-hint muted">
            {t(
              result.moreMatches.length === 1
                ? "result.moreMatchesHintOne"
                : "result.moreMatchesHintOther",
              { count: result.moreMatches.length },
            )}
          </span>
        ) : null}
      </span>
      {expanded && result.moreMatches.length > 0 ? (
        <span
          className="result-more-matches"
          aria-label={t("result.moreMatchesAriaLabel")}
        >
          {result.moreMatches.map((match) => (
            <span className="result-more-match" key={match.playbackChunkId}>
              <strong>
                <span className="mono">{match.timestamp}</span>
                <em className={`chip confidence-dot ${match.confidence}`}>
                  <span className="dot" />
                  {match.confidenceLabel} · {match.scoreLabel}
                </em>
              </strong>
              <span className="clamp2">{highlightSnippet(match.snippet, query)}</span>
            </span>
          ))}
        </span>
      ) : null}
    </article>
  );
}

export function ItemModalityIcon({ item, size }: { item: Item; size: number }) {
  if (item.contentType === "audio") {
    return <FileAudio size={size} />;
  }
  if (item.contentType === "image") {
    return <ImageIcon size={size} />;
  }
  if (item.contentType === "document") {
    return <FileText size={size} />;
  }
  return <FileVideo size={size} />;
}

export function ItemCard({
  item,
  viewMode = "grid",
  selectable = false,
  selected = false,
  transitionName,
  onSelect,
  onOpen,
}: {
  item: Item;
  viewMode?: "grid" | "list";
  selectable?: boolean;
  selected?: boolean;
  transitionName?: string;
  onSelect?: (selected: boolean) => void;
  onOpen: () => void;
}) {
  const t = useT();
  const metaLine = [
    item.source,
    item.indexedAtEpoch === null
      ? t("library.itemCard.notIndexed")
      : t("library.itemCard.indexedAt", { when: item.indexedAt }),
  ]
    .filter(Boolean)
    .join(" · ");
  const sourceLabel = `${itemKindLabel(item, t)} · ${item.source}`;
  const indexedCell =
    item.status === "indexing"
      ? t("library.status.indexing")
      : item.indexedAtEpoch === null
        ? "—"
        : item.indexedAt;
  const statusLabel = item.status === "failed"
    ? t("library.status.failed")
    : item.status === "indexing"
      ? (item.progressLabel ?? t("library.status.indexing"))
      : itemHasPartialIndex(item)
        ? t("library.itemCard.partialIndexShort")
        : null;
  const statusTone = item.status === "failed" ? "danger" : "warn";
  return (
    <article
      className={selected ? "item-card-shell lib-card selected" : "item-card-shell lib-card"}
      data-view={viewMode}
      style={transitionName ? { viewTransitionName: transitionName } : undefined}
    >
      {selectable ? (
        <label
          className="item-select"
          onClick={(event) => event.stopPropagation()}
        >
          <input
            type="checkbox"
            checked={selected}
            onChange={(event) => onSelect?.(event.currentTarget.checked)}
          />
          <span className="item-select-box" aria-hidden="true">
            {selected ? <Check size={15} strokeWidth={3} /> : null}
          </span>
          <span className="faint">{t("library.itemCard.selectAria")}</span>
        </label>
      ) : null}
      <button
        className={viewMode === "list" ? "item-card list" : "item-card"}
        type="button"
        aria-label={item.title}
        onClick={onOpen}
      >
        {viewMode === "list" ? (
          <>
            <span className="item-list-title">
              <span className={`item-thumb thumb ${item.thumbnailUrl ? "has-image" : item.color}`}>
                {item.thumbnailUrl ? (
                  <img src={item.thumbnailUrl} alt="" loading="lazy" />
                ) : (
                  <ItemModalityIcon item={item} size={15} />
                )}
              </span>
              <strong className="clamp1">{item.title}</strong>
            </span>
            <span className="item-list-cell item-list-source clamp1">{sourceLabel}</span>
            <span className="item-list-cell item-list-duration mono">{item.duration}</span>
            <span className="item-list-cell item-list-indexed">{indexedCell}</span>
            <span className={`item-list-cell item-list-status ${statusTone}`}>
              {statusLabel ? <><span className="dot" />{statusLabel}</> : null}
            </span>
          </>
        ) : (
          <>
            <span
              className={`item-thumb thumb ${
                item.status === "indexing"
                  ? "indexing"
                  : item.thumbnailUrl
                    ? "has-image"
                    : item.color
              }`}
            >
              {item.status === "indexing" ? (
                <>
                  {/* Elegant processing card (handoff §4): shimmer sweep over a
                      light gradient, a centred spinning ring, and a top-left
                      processing pill — instead of a bare modality icon. */}
                  <span className="item-shimmer" aria-hidden="true" />
                  <span className="item-ring" aria-hidden="true" />
                  <span className="item-proc-pill mono">
                    <span className="item-proc-dot" />
                    {t("library.status.indexing")}
                  </span>
                </>
              ) : item.thumbnailUrl ? (
                <img src={item.thumbnailUrl} alt="" loading="lazy" />
              ) : (
                <ItemModalityIcon item={item} size={22} />
              )}
              {item.contentType !== "image" && item.duration && item.status !== "indexing" ? (
                <small className="thumb-duration mono">{item.duration}</small>
              ) : null}
              {item.status === "failed" ? (
                <span className="item-state-pill danger"><span className="dot" />{t("library.status.failed")}</span>
              ) : itemHasPartialIndex(item) ? (
                <span className="item-state-pill warn"><span className="dot" />{t("library.itemCard.partialIndexShort")}</span>
              ) : null}
              {item.status === "indexing" && item.progress !== null ? (
                <span
                  className="item-progress-overlay"
                  aria-label={t("library.itemCard.progressAria", {
                    label: item.progressLabel ?? "",
                  }).trim()}
                >
                  <ProgressBar value={Math.round(item.progress * 100)} animated />
                  <small className="mono">
                    {[item.progressLabel ?? t("library.itemCard.indexingFallback"), item.etaLabel]
                      .filter(Boolean)
                      .join(" · ")}
                  </small>
                </span>
              ) : null}
            </span>
            <span className="item-copy body">
              <strong className="clamp2">{item.title}</strong>
              <span className="item-card-meta muted clamp1">{metaLine}</span>
            </span>
          </>
        )}
      </button>
    </article>
  );
}
