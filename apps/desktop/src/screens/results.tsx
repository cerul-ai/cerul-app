import { AlertTriangle, Check, ChevronDown, Search } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import type { KeyboardEvent, ReactNode } from "react";
import * as api from "../lib/api";
import { useT, type TFunction } from "../lib/i18n";
import { resultModality } from "../lib/results";
import type { ApiStatus, Result, ResultModalityFilter } from "../lib/types";
import { useClickOutside, useEscapeToClose } from "../lib/use-dismissable";
import { EmptyState } from "../components/leaf";
import { ResultCard } from "../components/cards";

type ResultsUiCache = {
  query: string;
  sortMode: "relevance" | "recent";
  sourceFilter: string;
  modalityFilter: ResultModalityFilter;
};

let resultsUiCache: ResultsUiCache | null = null;

export function ResultsScreen({
  query,
  rankingPreference,
  onRankingPreferenceChange,
  onRunQuery,
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
  rankingPreference: api.SearchRankingPreference;
  onRankingPreferenceChange: (value: api.SearchRankingPreference) => void;
  onRunQuery: (query: string) => void;
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
  const cachedUi = resultsUiCache?.query === query ? resultsUiCache : null;
  const previousQueryRef = useRef(query);
  const [selectedIndex, setSelectedIndex] = useState(0);
  const [expandedResultIds, setExpandedResultIds] = useState<Set<string>>(() => new Set());
  const [sortMode, setSortMode] = useState<"relevance" | "recent">(cachedUi?.sortMode ?? "relevance");
  const [sourceFilter, setSourceFilter] = useState(cachedUi?.sourceFilter ?? "all");
  const [modalityFilter, setModalityFilter] = useState<ResultModalityFilter>(cachedUi?.modalityFilter ?? "all");
  const [mobileQuery, setMobileQuery] = useState(query);

  const sourceOptions = useMemo(() => {
    const counts = new Map<string, number>();
    for (const result of results) counts.set(result.source, (counts.get(result.source) ?? 0) + 1);
    return [...counts.entries()].sort((left, right) => right[1] - left[1]);
  }, [results]);
  const modalityCounts = useMemo(() => ({
    all: results.length,
    video: results.filter((result) => resultModality(result) === "video").length,
    audio: results.filter((result) => resultModality(result) === "audio").length,
    image: results.filter((result) => resultModality(result) === "image").length,
    document: results.filter((result) => resultModality(result) === "document").length,
  }), [results]);
  const filtersActive =
    sourceFilter !== "all" ||
    modalityFilter !== "all" ||
    sortMode !== "relevance" ||
    rankingPreference !== "smart";
  const filteredResults = results.filter((result) =>
    (sourceFilter === "all" || result.source === sourceFilter) &&
    (modalityFilter === "all" || resultModality(result) === modalityFilter),
  );
  const displayedResults =
    sortMode === "recent"
      ? [...filteredResults].sort(
          (left, right) =>
            (right.indexedAtEpoch ?? 0) - (left.indexedAtEpoch ?? 0) ||
            right.rankScore - left.rankScore,
        )
      : [...filteredResults].sort((left, right) => right.rankScore - left.rankScore);
  const hasQuery = query.trim().length > 0;
  const hasSearched = hasQuery || results.length > 0;
  const diagnosticsText = diagnostics ? searchDiagnosticsSummary(diagnostics, t) : null;
  const diagnosticsTitle = diagnostics ? searchDiagnosticsTitle(diagnostics) : undefined;

  useEffect(() => {
    setSelectedIndex(0);
    setExpandedResultIds(new Set());
  }, [query, results.length, sourceFilter, modalityFilter, sortMode, rankingPreference]);

  useEffect(() => setMobileQuery(query), [query]);

  useEffect(() => {
    if (previousQueryRef.current !== query) {
      previousQueryRef.current = query;
      setSortMode("relevance");
      setSourceFilter("all");
      setModalityFilter("all");
      resultsUiCache = { query, sortMode: "relevance", sourceFilter: "all", modalityFilter: "all" };
      return;
    }
    resultsUiCache = { query, sortMode, sourceFilter, modalityFilter };
  }, [query, sortMode, sourceFilter, modalityFilter]);

  function focusResult(index: number) {
    window.requestAnimationFrame(() => {
      document.querySelector<HTMLElement>(`[data-result-index="${index}"]`)?.focus();
    });
  }

  function clearResultFilters() {
    setSourceFilter("all");
    setModalityFilter("all");
    setSortMode("relevance");
    onRankingPreferenceChange("smart");
  }

  function handleResultsKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    if (!displayedResults.length) return;
    if ((event.metaKey || event.ctrlKey) && event.key === "ArrowDown") {
      event.preventDefault();
      const selectedResult = displayedResults[Math.min(selectedIndex, displayedResults.length - 1)];
      if (selectedResult.moreMatches.length > 0) {
        setExpandedResultIds((current) => {
          const next = new Set(current);
          if (next.has(selectedResult.playbackChunkId)) next.delete(selectedResult.playbackChunkId);
          else next.add(selectedResult.playbackChunkId);
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
    <div className="page wide results-page-r1">
      <header className="results-r1-head">
        <div>
          <p className="page-eyebrow">{t("results.section.eyebrow")}</p>
          <h1>{query || t("results.heading")}</h1>
          <p>{isSearching ? t("results.status.searching") : t("results.localSummary", { count: displayedResults.length })}</p>
        </div>
        <div className="results-summary-tools">
          <span>{t("results.sort.label")}</span>
          <button type="button" className={sortMode === "relevance" ? "active" : ""} onClick={() => setSortMode("relevance")}>{t("results.sort.relevance")}</button>
          <button type="button" className={sortMode === "recent" ? "active" : ""} onClick={() => setSortMode("recent")}>{t("results.sort.recent")}</button>
          {filtersActive ? <button type="button" onClick={clearResultFilters}>{t("common.clearFilters")}</button> : null}
        </div>
      </header>

      <form
        className="results-mobile-search"
        role="search"
        onSubmit={(event) => {
          event.preventDefault();
          const nextQuery = mobileQuery.trim();
          if (nextQuery) onRunQuery(nextQuery);
        }}
      >
        <Search size={17} aria-hidden="true" />
        <input
          value={mobileQuery}
          onChange={(event) => setMobileQuery(event.target.value)}
          placeholder={t("results.searchPlaceholder")}
          aria-label={t("results.searchAria")}
        />
        <button type="submit" disabled={!mobileQuery.trim()}>{t("home.searchSubmit")}</button>
      </form>

      {error ? (
        <div className="state danger results-r1-error" role="alert">
          <div className="state-icon"><AlertTriangle size={18} /></div>
          <div className="state-sub">{error}</div>
        </div>
      ) : null}

      <div className="results-r1-layout">
        <aside className="results-filter-rail" aria-label={t("results.filter.sourceAria")}>
          <FilterGroup title={t("results.filter.sourceAria")}>
            <FilterButton active={sourceFilter === "all"} onClick={() => setSourceFilter("all")} label={t("results.filter.allSources")} count={results.length} />
            {sourceOptions.slice(0, 8).map(([source, count]) => (
              <FilterButton key={source} active={sourceFilter === source} onClick={() => setSourceFilter(source)} label={source} count={count} />
            ))}
          </FilterGroup>
          <FilterGroup title={t("results.filter.modalityAria")}>
            {(["all", "video", "audio", "image", "document"] as ResultModalityFilter[]).map((modality) => (
              <FilterButton
                key={modality}
                active={modalityFilter === modality}
                onClick={() => setModalityFilter(modality)}
                label={t(
                  modality === "all"
                    ? "results.modeTabs.all"
                    : modality === "document"
                      ? "results.modeTabs.documents"
                      : modality === "image"
                        ? "results.modeTabs.shown"
                        : `results.modeTabs.${modality}`,
                )}
                count={modalityCounts[modality]}
              />
            ))}
          </FilterGroup>
          <div className="results-ranking-select">
            <span>{t("results.preference.label")}</span>
            <RankingPreferenceMenu value={rankingPreference} onChange={onRankingPreferenceChange} />
          </div>
        </aside>

        <main
          className="results-card-list results-citation-stream"
          tabIndex={displayedResults.length ? 0 : undefined}
          onKeyDown={handleResultsKeyDown}
          aria-label={t("results.list.aria")}
        >
          {apiStatus !== "online" ? <p className="field-hint">{t("results.notice.demo")}</p> : null}
          {diagnosticsText && displayedResults.length === 0 ? <p className="field-hint" title={diagnosticsTitle}>{diagnosticsText}</p> : null}
          {isSearching ? <ResultsSkeletonList /> : null}
          {!isSearching && displayedResults.length > 0 ? displayedResults.map((result, index) => (
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
          )) : null}
          {!isSearching && displayedResults.length === 0 ? (
            !hasSearched ? <EmptyState title={t("results.empty.initial.title")} body={t("results.empty.initial.body")} /> : (
              <EmptyState
                title={results.length > 0 && filtersActive ? t("results.empty.filtered.title") : !hasIndexedItems && hasActiveJobs ? t("results.empty.indexing.title") : t("results.empty.none.title")}
                body={results.length > 0 && filtersActive ? t("results.empty.filtered.body") : !hasIndexedItems && hasActiveJobs ? t("results.empty.indexing.body") : t("results.empty.none.body")}
                actionLabel={filtersActive ? t("common.clearFilters") : undefined}
                onAction={filtersActive ? clearResultFilters : undefined}
              />
            )
          ) : null}
        </main>

      </div>
    </div>
  );
}

const RANKING_PREFERENCES: api.SearchRankingPreference[] = [
  "smart",
  "video",
  "image",
  "document",
  "audio",
];

function RankingPreferenceMenu({
  value,
  onChange,
}: {
  value: api.SearchRankingPreference;
  onChange: (value: api.SearchRankingPreference) => void;
}) {
  const t = useT();
  const [open, setOpen] = useState(false);
  const [activeIndex, setActiveIndex] = useState(() =>
    Math.max(0, RANKING_PREFERENCES.indexOf(value)),
  );
  const rootRef = useRef<HTMLDivElement | null>(null);
  useEscapeToClose(() => setOpen(false), open);
  useClickOutside(rootRef, () => setOpen(false), open);

  useEffect(() => {
    setActiveIndex(Math.max(0, RANKING_PREFERENCES.indexOf(value)));
  }, [value]);

  const label = (preference: api.SearchRankingPreference) =>
    t(`results.preference.${preference}`);

  function choose(preference: api.SearchRankingPreference) {
    setOpen(false);
    if (preference !== value) onChange(preference);
  }

  function handleKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      const direction = event.key === "ArrowDown" ? 1 : -1;
      setOpen(true);
      setActiveIndex((current) =>
        (current + direction + RANKING_PREFERENCES.length) % RANKING_PREFERENCES.length,
      );
      return;
    }
    if (open && event.key === "Enter") {
      event.preventDefault();
      choose(RANKING_PREFERENCES[activeIndex]);
    }
    if (event.key === "Tab") setOpen(false);
  }

  return (
    <div
      className={open ? "model-combobox results-ranking-menu open" : "model-combobox results-ranking-menu"}
      ref={rootRef}
      onKeyDown={handleKeyDown}
    >
      <button
        type="button"
        className="model-combobox__field"
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-label={t("results.preference.aria")}
        onClick={() => setOpen((current) => !current)}
      >
        <span className="model-combobox__value">{label(value)}</span>
        <ChevronDown size={15} className="model-combobox__chev" />
      </button>
      {open ? (
        <div className="model-combobox__pop">
          <div className="model-combobox__list" role="listbox" aria-label={t("results.preference.aria")}>
            {RANKING_PREFERENCES.map((preference, index) => (
              <button
                type="button"
                key={preference}
                className={index === activeIndex ? "model-combobox__opt active" : "model-combobox__opt"}
                role="option"
                aria-selected={preference === value}
                onMouseEnter={() => setActiveIndex(index)}
                onClick={() => choose(preference)}
              >
                <span className="model-combobox__opt-id">{label(preference)}</span>
                {preference === value ? <Check size={14} aria-hidden="true" /> : null}
              </button>
            ))}
          </div>
        </div>
      ) : null}
    </div>
  );
}

function FilterGroup({ title, children }: { title: string; children: ReactNode }) {
  return <section className="results-filter-group"><h2>{title}</h2>{children}</section>;
}

function FilterButton({ active, onClick, label, count }: { active: boolean; onClick: () => void; label: string; count: number }) {
  return <button type="button" className={active ? "active" : ""} onClick={onClick}><span className="clamp1">{label}</span><code>{count}</code></button>;
}

function ResultsSkeletonList() {
  return <>{[0, 1, 2].map((index) => <div className="result-row result-skeleton" key={index} aria-hidden="true"><span className="sk" style={{ width: 132, height: 74, borderRadius: "var(--r-md)" }} /><span className="col gap-2" style={{ paddingTop: 4 }}><span className="sk" style={{ height: 13, width: "70%" }} /><span className="sk" style={{ height: 11, width: "92%" }} /><span className="sk" style={{ height: 11, width: "55%" }} /></span></div>)}</>;
}

function searchDiagnosticsSummary(diagnostics: api.SearchDiagnostics, t: TFunction) {
  const base = t("results.diagnostics.summary", { mode: searchRetrievalModeLabel(diagnostics.retrieval_mode, t), vector: diagnostics.vector_hits_count, fts: diagnostics.fts_hits_count });
  if (!diagnostics.fallback_reason) return base;
  return `${base} · ${t("results.diagnostics.reason", { reason: searchFallbackReasonLabel(diagnostics.fallback_reason, t) })}`;
}

function searchRetrievalModeLabel(mode: string, t: TFunction) {
  switch (mode) {
    case "unified_vector": return t("results.diagnostics.mode.unifiedVector");
    case "hybrid": return t("results.diagnostics.mode.hybrid");
    case "vector": return t("results.diagnostics.mode.vector");
    case "fts": return t("results.diagnostics.mode.fts");
    case "fts_fallback": return t("results.diagnostics.mode.ftsFallback");
    case "empty": return t("results.diagnostics.mode.empty");
    default: return mode;
  }
}

function searchFallbackReasonLabel(reason: string, t: TFunction) {
  switch (reason) {
    case "embedding_unavailable":
    case "query_embedding_failed": return t("results.diagnostics.reason.queryEmbeddingFailed");
    case "query_embedding_task_failed": return t("results.diagnostics.reason.queryEmbeddingTaskFailed");
    case "query_embedding_timeout": return t("results.diagnostics.reason.queryEmbeddingTimeout");
    case "vector_search_failed": return t("results.diagnostics.reason.vectorSearchFailed");
    case "vector_index_empty":
    case "unified_vector_index_empty": return t("results.diagnostics.reason.vectorIndexEmpty");
    case "no_vector_hits":
    case "no_unified_vector_hits": return t("results.diagnostics.reason.noVectorHits");
    case "search_index_rebuilding_legacy_fts": return t("results.diagnostics.reason.searchIndexRebuildingLegacyFts");
    case "vector_index_unavailable": return t("results.diagnostics.reason.vectorIndexUnavailable");
    default: return reason;
  }
}

function searchDiagnosticsTitle(diagnostics: api.SearchDiagnostics) {
  return [`profile=${diagnostics.embedding_profile_id ?? "-"}`, `collection=${diagnostics.vector_index_collection ?? "-"}`, `points=${diagnostics.vector_index_point_count ?? "-"}`, `units=${diagnostics.retrieval_unit_count ?? "-"}`, `indexed_items=${diagnostics.indexed_item_count ?? "-"}`, `needs_rebuild=${diagnostics.items_needing_rebuild ?? "-"}`].join(" ");
}
