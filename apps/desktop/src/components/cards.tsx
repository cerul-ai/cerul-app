// Result and library item cards. Extracted from App.tsx (B13 Phase B).
//
// Pure props-driven. ResultCard renders a single ranked search result
// with the matched-text highlight — the most important affordance
// for a search result. ItemCard renders a library entry
// with an optional selection checkbox and an indexing progress
// overlay.

import {
  Eye,
  FileAudio,
  FileVideo,
  Gauge,
  Image as ImageIcon,
  Mic,
  Play,
  Sparkles,
} from "lucide-react";
import { useT, type TFunction } from "../lib/i18n";
import { formatUsd } from "../lib/formatters";
import { itemKindLabel } from "../lib/items";
import { resultModality } from "../lib/results";
import type { Item, Result } from "../lib/types";
import { ProgressBar, highlightSnippet } from "./transcript";

// Single searchability chip summarising an item's state, mirroring the
// redesign baseline (语音 + 画面可搜 / 仅语音可搜 / 索引中 · % / 处理失败).
/** Just the searchable-modality label (speech / visual), independent of index
 * status — for the detail subtitle, where indexed/failed status is shown
 * separately. itemSearchability() below folds status in for library cards. */
export function itemModalityLabel(item: Item, t: TFunction): string {
  const hasVisual =
    item.contentType === "image" ||
    (item.contentType === "video" && item.visualIndexStatus === "indexed");
  const hasSpeech = item.contentType === "video" || item.contentType === "audio";
  if (hasVisual && hasSpeech) {
    return t("library.itemCard.searchSpeechVisual");
  }
  if (hasVisual) {
    return t("library.itemCard.searchVisualOnly");
  }
  return t("library.itemCard.searchSpeechOnly");
}

function itemSearchability(
  item: Item,
  t: TFunction,
): { label: string; tone: "accent" | "warn" | "danger" } {
  if (item.status === "failed") {
    return { label: t("library.itemCard.failedClick"), tone: "danger" };
  }
  if (item.status === "indexing") {
    const pct =
      item.progressLabel ??
      (item.progress !== null ? `${Math.round(item.progress * 100)}%` : null);
    return {
      label: pct ? t("library.itemCard.indexingPct", { pct }) : t("library.status.indexing"),
      tone: "warn",
    };
  }
  // A failed embedding index means semantic/vector search is incomplete even
  // though the item is otherwise indexed — surface that, don't claim it's fully
  // searchable.
  if (item.embeddingIndexStatus === "failed") {
    return { label: t("library.itemCard.partialIndex"), tone: "warn" };
  }
  // Visual search is real only once the visual index is actually indexed
  // (pending/null is not searchable yet); images are inherently visual.
  const hasVisual =
    item.contentType === "image" ||
    (item.contentType === "video" && item.visualIndexStatus === "indexed");
  const hasSpeech = item.contentType === "video" || item.contentType === "audio";
  if (hasVisual && hasSpeech) {
    return { label: t("library.itemCard.searchSpeechVisual"), tone: "accent" };
  }
  if (hasVisual) {
    return { label: t("library.itemCard.searchVisualOnly"), tone: "accent" };
  }
  return { label: t("library.itemCard.searchSpeechOnly"), tone: "warn" };
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
        : t("result.modality.both");
  const ModalityBadgeIcon =
    modality === "audio" ? Mic : modality === "image" ? Eye : Sparkles;

  return (
    <button
      className={className}
      type="button"
      data-result-index={index}
      aria-selected={selected}
      aria-expanded={result.moreMatches.length > 0 ? expanded : undefined}
      onFocus={onFocus}
      onClick={() => onOpen(result)}
    >
      <span className={`thumb ${result.thumbnailUrl ? "has-image" : result.color}`}>
        {result.thumbnailUrl ? (
          <img src={result.thumbnailUrl} alt="" loading="lazy" />
        ) : (
          <>
            <Play size={24} fill="currentColor" />
            <small className="mono">{result.timestamp}</small>
          </>
        )}
      </span>
      <span className="result-body">
        <span className="result-meta">
          <ResultModalityIcon result={result} size={14} />
          <span className="chip neutral result-source-label">
            <span className="dot" />
            {result.source}
          </span>
          <em className={`chip modality-pill ${modality}`}>
            <span className="dot" />
            <ModalityBadgeIcon size={14} />
            {modalityLabel}
          </em>
          <em className={`chip confidence-pill ${result.confidence}`}>
            <span className="dot" />
            <Gauge size={14} />
            {result.confidenceLabel}
          </em>
          <em className="chip score-pill mono" title={result.scoreTitle}>
            {result.scoreLabel}
          </em>
        </span>
        <strong className="clamp1">{result.title}</strong>
        <span className="snippet clamp2">
          {highlightSnippet(result.snippet, query)}
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
      <span className="timestamp mono">
        {result.timestamp}
        <small>{result.duration}</small>
      </span>
      {expanded && result.moreMatches.length > 0 ? (
        <span
          className="result-more-matches"
          aria-label={t("result.moreMatchesAriaLabel")}
        >
          {result.moreMatches.map((match) => (
            <span className="result-more-match" key={match.id}>
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
    </button>
  );
}

export function ItemModalityIcon({ item, size }: { item: Item; size: number }) {
  if (item.contentType === "audio") {
    return <FileAudio size={size} />;
  }
  if (item.contentType === "image") {
    return <ImageIcon size={size} />;
  }
  return <FileVideo size={size} />;
}

export function ItemCard({
  item,
  viewMode = "grid",
  selectable = false,
  selected = false,
  onSelect,
  onOpen,
}: {
  item: Item;
  viewMode?: "grid" | "list";
  selectable?: boolean;
  selected?: boolean;
  onSelect?: (selected: boolean) => void;
  onOpen: () => void;
}) {
  const t = useT();
  const searchability = itemSearchability(item, t);
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
  const searchabilityChip = (
    <span className={`item-searchability chip ${searchability.tone}`}>
      <span className="dot" />
      {searchability.label}
    </span>
  );
  return (
    <article
      className={selected ? "item-card-shell lib-card selected" : "item-card-shell lib-card"}
      data-view={viewMode}
    >
      {selectable ? (
        <label className="item-select sel-check">
          <input
            type="checkbox"
            checked={selected}
            onChange={(event) => onSelect?.(event.currentTarget.checked)}
          />
          <span className="faint">{t("library.itemCard.selectAria")}</span>
        </label>
      ) : null}
      <button
        className={viewMode === "list" ? "item-card list" : "item-card"}
        type="button"
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
            <span className="item-list-cell item-list-search">{searchabilityChip}</span>
          </>
        ) : (
          <>
            <span className={`item-thumb thumb ${item.thumbnailUrl ? "has-image" : item.color}`}>
              {item.thumbnailUrl ? (
                <img src={item.thumbnailUrl} alt="" loading="lazy" />
              ) : (
                <ItemModalityIcon item={item} size={22} />
              )}
              {item.contentType !== "image" && item.duration && item.status !== "indexing" ? (
                <small className="thumb-duration mono">{item.duration}</small>
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
              {item.usage.event_count > 0 ? (
                <span className="item-usage mono muted">
                  {formatUsd(item.usage.estimated_usd)} ·{" "}
                  {t(
                    item.usage.event_count === 1
                      ? "library.itemCard.usageEventOne"
                      : "library.itemCard.usageEventOther",
                    { count: item.usage.event_count },
                  )}
                </span>
              ) : null}
              {searchabilityChip}
            </span>
          </>
        )}
      </button>
    </article>
  );
}
