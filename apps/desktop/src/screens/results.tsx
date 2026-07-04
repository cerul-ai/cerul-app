import { AlertTriangle, ChevronRight, Search } from "lucide-react";
import { useEffect, useState } from "react";
import type { FormEvent, KeyboardEvent } from "react";
import * as api from "../lib/api";
import { useT, type TFunction } from "../lib/i18n";
import { resultModality } from "../lib/results";
import { submitSearchInputOnEnter } from "../lib/route";
import type { ApiStatus, Result, ResultModalityFilter } from "../lib/types";
import { EmptyState } from "../components/leaf";
import { ResultCard } from "../components/cards";

export function ResultsScreen({
  query,
  setQuery,
  onSubmit,
  onBack,
  onOpen,
  results,
  diagnostics,
  isSearching,
  error,
  apiStatus,
  hasIndexedItems,
  hasActiveJobs,
}: {
  query: string;
  setQuery: (value: string) => void;
  onSubmit: (event: FormEvent<HTMLFormElement>) => void;
  onBack: () => void;
  onOpen: (result: Result) => void;
  results: Result[];
  diagnostics: api.SearchDiagnostics | null;
  isSearching: boolean;
  error: string | null;
  apiStatus: ApiStatus;
  hasIndexedItems: boolean;
  hasActiveJobs: boolean;
}) {
  const t = useT();
  const [selectedIndex, setSelectedIndex] = useState(0);
  const [expandedResultIds, setExpandedResultIds] = useState<Set<string>>(() => new Set());
  const [modalityFilter, setModalityFilter] = useState<ResultModalityFilter>("all");
  const [sortMode, setSortMode] = useState<"relevance" | "recent">("relevance");
  const filtersActive =
    modalityFilter !== "all" || sortMode !== "relevance";
  const filteredResults = results.filter((result) => {
    const matchesModality = modalityFilter === "all" || resultModality(result) === modalityFilter;
    return matchesModality;
  });
  const displayedResults =
    sortMode === "recent"
      ? [...filteredResults].sort(
          (left, right) =>
            (right.indexedAtEpoch ?? 0) - (left.indexedAtEpoch ?? 0) ||
            right.rankScore - left.rankScore,
        )
      : [...filteredResults].sort((left, right) => right.rankScore - left.rankScore);
  const modalityCounts = {
    all: results.length,
    audio: results.filter((result) => resultModality(result) === "audio").length,
    document: results.filter((result) => resultModality(result) === "document").length,
    image: results.filter((result) => resultModality(result) === "image").length,
    video: results.filter((result) => resultModality(result) === "video").length,
  };
  const hasQuery = query.trim().length > 0;
  const hasSearched = hasQuery || results.length > 0;
  const diagnosticsText = diagnostics ? searchDiagnosticsSummary(diagnostics, t) : null;
  const diagnosticsTitle = diagnostics ? searchDiagnosticsTitle(diagnostics) : undefined;

  useEffect(() => {
    setSelectedIndex(0);
    setExpandedResultIds(new Set());
  }, [query, results.length, modalityFilter, sortMode]);

  function focusResult(index: number) {
    window.requestAnimationFrame(() => {
      document.querySelector<HTMLElement>(`[data-result-index="${index}"]`)?.focus();
    });
  }

  function clearResultFilters() {
    setModalityFilter("all");
    setSortMode("relevance");
  }

  function handleResultsKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    if (!displayedResults.length) {
      return;
    }

    if ((event.metaKey || event.ctrlKey) && event.key === "ArrowDown") {
      event.preventDefault();
      const selectedResult = displayedResults[Math.min(selectedIndex, displayedResults.length - 1)];
      if (selectedResult.moreMatches.length > 0) {
        setExpandedResultIds((current) => {
          const next = new Set(current);
          if (next.has(selectedResult.playbackChunkId)) {
            next.delete(selectedResult.playbackChunkId);
          } else {
            next.add(selectedResult.playbackChunkId);
          }
          return next;
        });
      }
      return;
    }

    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      const direction = event.key === "ArrowDown" ? 1 : -1;
      const nextIndex = (selectedIndex + direction + displayedResults.length) % displayedResults.length;
      setSelectedIndex(nextIndex);
      focusResult(nextIndex);
    }

    if (event.key === "Enter" && event.target === event.currentTarget) {
      event.preventDefault();
      onOpen(displayedResults[Math.min(selectedIndex, displayedResults.length - 1)]);
    }
  }

  return (
    <>
      <div className="topbar">
        <div className="tb-inner">
          <button className="btn-icon" type="button" onClick={onBack} aria-label={t("results.backHome")}>
            <ChevronRight size={16} style={{ transform: "rotate(180deg)" }} />
          </button>
          <form className="search-wrap" onSubmit={onSubmit} style={{ flex: 1, maxWidth: 480 }}>
            <Search size={16} style={{ left: 12, width: 16, height: 16 }} />
            <input
              className="input"
              name="query"
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              onKeyDown={submitSearchInputOnEnter}
              placeholder={t("results.searchPlaceholder")}
              aria-label={t("results.searchAria")}
              style={{ height: 38, paddingLeft: 38 }}
            />
          </form>
          <span className="muted mono" style={{ fontSize: 12, marginLeft: "auto" }}>
            {t("results.status.hits", { count: displayedResults.length })}
          </span>
        </div>
      </div>

      <div className="page">
        <div className="row results-filter-row">
          <div className="segmented" aria-label={t("results.modeTabs.aria")}>
            <button
              type="button"
              className={modalityFilter === "all" ? "active" : ""}
              onClick={() => setModalityFilter("all")}
            >
              {t("results.modeTabs.all")} <span className="chip neutral">{modalityCounts.all}</span>
            </button>
            <button
              type="button"
              className={modalityFilter === "video" ? "active" : ""}
              onClick={() => setModalityFilter("video")}
            >
              {t("results.modeTabs.video")} <span className="chip neutral">{modalityCounts.video}</span>
            </button>
            <button
              type="button"
              className={modalityFilter === "image" ? "active" : ""}
              onClick={() => setModalityFilter("image")}
            >
              {t("results.modeTabs.shown")} <span className="chip neutral">{modalityCounts.image}</span>
            </button>
            <button
              type="button"
              className={modalityFilter === "audio" ? "active" : ""}
              onClick={() => setModalityFilter("audio")}
            >
              {t("results.modeTabs.audio")} <span className="chip neutral">{modalityCounts.audio}</span>
            </button>
            <button
              type="button"
              className={modalityFilter === "document" ? "active" : ""}
              onClick={() => setModalityFilter("document")}
            >
              {t("results.modeTabs.documents")} <span className="chip neutral">{modalityCounts.document}</span>
            </button>
          </div>
          <div className="row gap-2">
            <span className="muted" style={{ fontSize: 12.5 }}>{t("results.sort.label")}</span>
            <div className="segmented">
              <button
                type="button"
                className={sortMode === "relevance" ? "active" : ""}
                onClick={() => setSortMode("relevance")}
              >
                {t("results.sort.relevance")}
              </button>
              <button
                type="button"
                className={sortMode === "recent" ? "active" : ""}
                onClick={() => setSortMode("recent")}
              >
                {t("results.sort.recent")}
              </button>
            </div>
          </div>
        </div>

        {error ? (
          <div className="state danger" role="alert" style={{ marginTop: 12 }}>
            <div className="state-icon">
              <AlertTriangle size={18} />
            </div>
            <div className="state-sub">{error}</div>
          </div>
        ) : null}
        {apiStatus !== "online" ? (
          <p className="field-hint" style={{ marginTop: 10 }}>
            {t("results.notice.demo")}
          </p>
        ) : null}
        {hasSearched && !isSearching ? (
          <div className="row" style={{ alignItems: "center", gap: 10, marginTop: 12 }}>
            <span className="muted">
              {t("results.summary.count", {
                count: displayedResults.length,
                total: results.length,
              })}
            </span>
            {filtersActive ? (
              <button type="button" className="btn btn-ghost sm" onClick={clearResultFilters}>
                {t("common.clearFilters")}
              </button>
            ) : null}
          </div>
        ) : null}
        {diagnosticsText ? (
          <p className="field-hint" style={{ marginTop: 6 }} title={diagnosticsTitle}>
            {diagnosticsText}
          </p>
        ) : null}

        <div
          className={displayedResults.length > 0 || isSearching ? "card results-card-list" : "results-card-list"}
          tabIndex={displayedResults.length ? 0 : undefined}
          onKeyDown={handleResultsKeyDown}
          aria-label={t("results.list.aria")}
        >
          {isSearching ? <ResultsSkeletonList /> : null}
          {!isSearching && displayedResults.length > 0
            ? displayedResults.map((result, index) => (
              <ResultCard
                key={result.playbackChunkId}
                result={result}
                index={index}
                selected={index === selectedIndex}
                expanded={expandedResultIds.has(result.playbackChunkId)}
                onFocus={() => setSelectedIndex(index)}
                onOpen={onOpen}
                query={query}
              />
            ))
            : null}
          {!isSearching && displayedResults.length === 0 ? (
            !hasSearched ? (
              <EmptyState
                title={t("results.empty.initial.title")}
                body={t("results.empty.initial.body")}
              />
            ) : (
              <EmptyState
                title={
                  results.length > 0 && filtersActive
                    ? t("results.empty.filtered.title")
                    : !hasIndexedItems && hasActiveJobs
                      ? t("results.empty.indexing.title")
                      : t("results.empty.none.title")
                }
                body={
                  results.length > 0 && filtersActive
                    ? t("results.empty.filtered.body")
                    : !hasIndexedItems && hasActiveJobs
                      ? t("results.empty.indexing.body")
                      : t("results.empty.none.body")
                }
              />
            )
          ) : null}
        </div>
      </div>
    </>
  );
}

function ResultsSkeletonList() {
  return (
    <>
      {[0, 1, 2].map((index) => (
        <div className="result-row result-skeleton" key={index} aria-hidden="true">
          <span className="sk" style={{ width: 132, height: 74, borderRadius: "var(--r-md)" }} />
          <span className="col gap-2" style={{ paddingTop: 4 }}>
            <span className="sk" style={{ height: 13, width: "70%" }} />
            <span className="sk" style={{ height: 11, width: "92%" }} />
            <span className="sk" style={{ height: 11, width: "55%" }} />
          </span>
          <span className="sk" style={{ height: 11, width: 44 }} />
        </div>
      ))}
    </>
  );
}

function searchDiagnosticsSummary(diagnostics: api.SearchDiagnostics, t: TFunction) {
  const base = t("results.diagnostics.summary", {
    mode: searchRetrievalModeLabel(diagnostics.retrieval_mode, t),
    vector: diagnostics.vector_hits_count,
    fts: diagnostics.fts_hits_count,
  });
  if (!diagnostics.fallback_reason) {
    return base;
  }
  return `${base} · ${t("results.diagnostics.reason", {
    reason: searchFallbackReasonLabel(diagnostics.fallback_reason, t),
  })}`;
}

function searchRetrievalModeLabel(mode: string, t: TFunction) {
  switch (mode) {
    case "unified_vector":
      return t("results.diagnostics.mode.unifiedVector");
    case "hybrid":
      return t("results.diagnostics.mode.hybrid");
    case "vector":
      return t("results.diagnostics.mode.vector");
    case "fts":
      return t("results.diagnostics.mode.fts");
    case "fts_fallback":
      return t("results.diagnostics.mode.ftsFallback");
    case "empty":
      return t("results.diagnostics.mode.empty");
    default:
      return mode;
  }
}

function searchFallbackReasonLabel(reason: string, t: TFunction) {
  switch (reason) {
    case "embedding_unavailable":
    case "query_embedding_failed":
      return t("results.diagnostics.reason.queryEmbeddingFailed");
    case "query_embedding_task_failed":
      return t("results.diagnostics.reason.queryEmbeddingTaskFailed");
    case "query_embedding_timeout":
      return t("results.diagnostics.reason.queryEmbeddingTimeout");
    case "vector_search_failed":
      return t("results.diagnostics.reason.vectorSearchFailed");
    case "vector_index_empty":
    case "unified_vector_index_empty":
      return t("results.diagnostics.reason.vectorIndexEmpty");
    case "no_vector_hits":
    case "no_unified_vector_hits":
      return t("results.diagnostics.reason.noVectorHits");
    case "search_index_rebuilding_legacy_fts":
      return t("results.diagnostics.reason.searchIndexRebuildingLegacyFts");
    case "vector_index_unavailable":
      return t("results.diagnostics.reason.vectorIndexUnavailable");
    default:
      return reason;
  }
}

function searchDiagnosticsTitle(diagnostics: api.SearchDiagnostics) {
  return [
    `profile=${diagnostics.embedding_profile_id ?? "-"}`,
    `collection=${diagnostics.vector_index_collection ?? "-"}`,
    `points=${diagnostics.vector_index_point_count ?? "-"}`,
    `units=${diagnostics.retrieval_unit_count ?? "-"}`,
    `indexed_items=${diagnostics.indexed_item_count ?? "-"}`,
    `needs_rebuild=${diagnostics.items_needing_rebuild ?? "-"}`,
  ].join(" ");
}
