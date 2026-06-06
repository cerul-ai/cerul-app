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
import { useT } from "../lib/i18n";
import { formatUsd } from "../lib/formatters";
import { resultModality } from "../lib/results";
import type { Item, Result } from "../lib/types";
import { ProgressBar, StatusBadge, highlightSnippet } from "./transcript";

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
  const statusLabel =
    item.status === "indexed"
      ? t("library.status.indexed")
      : item.status === "indexing"
        ? t("library.status.indexing")
        : t("library.status.failed");
  return (
    <article
      className={
        selected ? "item-card-shell lib-card selected" : "item-card-shell lib-card"
      }
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
        <span className={`item-thumb thumb ${item.thumbnailUrl ? "has-image" : item.color}`}>
          {item.thumbnailUrl ? (
            <img src={item.thumbnailUrl} alt="" loading="lazy" />
          ) : (
            <ItemModalityIcon item={item} size={22} />
          )}
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
          <span className="muted">{item.source}</span>
          <span className="muted">
            {item.duration} ·{" "}
            {item.indexedAt === "Never"
              ? t("library.itemCard.notIndexed")
              : t("library.itemCard.indexedAt", { when: item.indexedAt })}
          </span>
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
          {item.visualIndexStatus === "failed" ? (
            <span className="item-warning chip warn">
              <span className="dot" />
              {t("library.itemCard.transcriptOnly")}
            </span>
          ) : null}
          {item.embeddingIndexStatus === "failed" ? (
            <span className="item-warning chip warn">
              <span className="dot" />
              {t("library.itemCard.partialIndex")}
            </span>
          ) : null}
        </span>
        <StatusBadge status={item.status} label={statusLabel} />
      </button>
    </article>
  );
}
