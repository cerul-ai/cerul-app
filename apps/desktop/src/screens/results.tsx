import { AlertTriangle, Loader2, RefreshCcw, Sparkles } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import type { FormEvent, KeyboardEvent, ReactNode } from "react";
import * as api from "../lib/api";
import { errorMessage } from "../lib/formatters";
import { useLang, useT, type TFunction } from "../lib/i18n";
import { buildFollowupQuestion, resultModality } from "../lib/results";
import type { ApiStatus, Result, ResultModalityFilter } from "../lib/types";
import { EmptyState } from "../components/leaf";
import { ResultCard } from "../components/cards";

type AnswerState = "idle" | "loading" | "ready" | "error";

export function ResultsScreen({
  query,
  rankingPreference,
  onRankingPreferenceChange,
  onOpen,
  onOpenCitation,
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
  onOpen: (result: Result) => void;
  onOpenCitation: (citation: api.AskCitation) => void;
  results: Result[];
  diagnostics: api.SearchDiagnostics | null;
  isSearching: boolean;
  error: string | null;
  apiStatus: ApiStatus;
  hasIndexedItems: boolean;
  hasActiveJobs: boolean;
}) {
  const t = useT();
  const { lang } = useLang();
  const answerRequest = useRef(0);
  const [selectedIndex, setSelectedIndex] = useState(0);
  const [expandedResultIds, setExpandedResultIds] = useState<Set<string>>(() => new Set());
  const [sortMode, setSortMode] = useState<"relevance" | "recent">("relevance");
  const [sourceFilter, setSourceFilter] = useState("all");
  const [modalityFilter, setModalityFilter] = useState<ResultModalityFilter>("all");
  const [answerState, setAnswerState] = useState<AnswerState>("idle");
  const [answer, setAnswer] = useState<api.AskResponse | null>(null);
  const [answerError, setAnswerError] = useState<string | null>(null);
  const [followup, setFollowup] = useState("");

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

  async function requestAnswer(question: string) {
    const normalized = question.trim();
    if (!normalized || apiStatus !== "online") return;
    const requestId = ++answerRequest.current;
    setAnswerState("loading");
    setAnswerError(null);
    try {
      const ask = api.isAgentExperienceEnabled() ? api.askAgentLibrary : api.askLibrary;
      const next = await ask(normalized, 6, lang);
      if (answerRequest.current !== requestId) return;
      setAnswer(next);
      setAnswerState("ready");
    } catch (askError) {
      if (answerRequest.current !== requestId) return;
      setAnswer(null);
      setAnswerError(errorMessage(askError));
      setAnswerState("error");
    }
  }

  useEffect(() => {
    setSelectedIndex(0);
    setExpandedResultIds(new Set());
  }, [query, results.length, sourceFilter, modalityFilter, sortMode, rankingPreference]);

  useEffect(() => {
    if (!query.trim() || results.length === 0 || isSearching || apiStatus !== "online") {
      answerRequest.current += 1;
      setAnswer(null);
      setAnswerState("idle");
      setAnswerError(null);
      return;
    }
    const timer = window.setTimeout(() => void requestAnswer(query), 220);
    return () => window.clearTimeout(timer);
  }, [query, results.length, isSearching, apiStatus, lang]);

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

  function submitFollowup(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!followup.trim()) return;
    const contextualQuestion = buildFollowupQuestion(query, followup, displayedResults, answer);
    setFollowup("");
    void requestAnswer(contextualQuestion);
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
          <label className="results-ranking-select">
            <span>{t("results.preference.label")}</span>
            <select value={rankingPreference} onChange={(event) => onRankingPreferenceChange(event.currentTarget.value as api.SearchRankingPreference)}>
              <option value="smart">{t("results.preference.smart")}</option>
              <option value="video">{t("results.preference.video")}</option>
              <option value="image">{t("results.preference.image")}</option>
              <option value="document">{t("results.preference.document")}</option>
              <option value="audio">{t("results.preference.audio")}</option>
            </select>
          </label>
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

        <aside className="results-answer-rail" aria-label={t("results.answer.title")}>
          <header>
            <span><Sparkles size={13} />{t("results.answer.title")}</span>
            <button type="button" aria-label={t("results.answer.retry")} disabled={!query.trim() || answerState === "loading"} onClick={() => void requestAnswer(query)}><RefreshCcw size={13} /></button>
          </header>
          <p className="results-answer-grounding">{t("results.answer.grounding", { count: results.length })}</p>
          {answerState === "loading" ? <div className="results-answer-loading"><Loader2 size={18} className="spin" /><span>{t("results.answer.loading")}</span></div> : null}
          {answerState === "error" ? <div className="results-answer-error"><strong>{t("overlay.error.title")}</strong><span>{answerError}</span></div> : null}
          {answerState === "idle" ? <div className="results-answer-empty"><strong>{t("overlay.ask.emptyTitle")}</strong><span>{t("overlay.ask.emptyBody")}</span></div> : null}
          {answerState === "ready" && answer ? (
            <div className="results-answer-content">
              <p>{answer.answer}</p>
              <div className="results-answer-citations">
                {answer.citations.map((citation, index) => (
                  <button key={citation.playback_chunk_id} type="button" onClick={() => onOpenCitation(citation)}>
                    <code>{index + 1} · {citation.timestamp}</code>
                    <strong className="clamp1">{citation.title}</strong>
                    <span className="clamp2">{citation.snippet}</span>
                  </button>
                ))}
              </div>
            </div>
          ) : null}
          <form className="results-followup" onSubmit={submitFollowup}>
            <input value={followup} onChange={(event) => setFollowup(event.currentTarget.value)} placeholder={t("results.answer.followup")} />
            <button type="submit" disabled={!followup.trim() || answerState === "loading"}>{t("results.answer.ask")}</button>
          </form>
        </aside>
      </div>
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
